use log::debug;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, ToSocketAddrs};
use std::net::TcpStream;
use std::time::Duration;
use std::time::Instant;
use std::{fmt, io::Cursor};

use chunked_transfer::Decoder as ChunkDecoder;

#[cfg(feature = "tls")]
use rustls::ClientConnection;
#[cfg(feature = "tls")]
use rustls::StreamOwned;

use crate::{error::Error};

use crate::error::ErrorKind;
use crate::unit::Unit;

#[allow(clippy::large_enum_variant)]
pub(crate) enum Stream {
    Http(TcpStream),
    #[cfg(feature = "tls")]
    Https(rustls::StreamOwned<rustls::ClientConnection, TcpStream>),
    Test(Box<dyn Read + Send + Sync>, Vec<u8>),
}

// DeadlineStream wraps a stream such that read() will return an error
// after the provided deadline, and sets timeouts on the underlying
// TcpStream to ensure read() doesn't block beyond the deadline.
// When the From trait is used to turn a DeadlineStream back into a
// Stream (by PoolReturningRead), the timeouts are removed.

// If the deadline is in the future, return the remaining time until
// then. Otherwise return a TimedOut error.
pub(crate) fn time_until_deadline(deadline: Instant) -> io::Result<Duration> {
    let now = Instant::now();
    match deadline.checked_duration_since(now) {
        None => Err(io_err_timeout("timed out reading response".to_string())),
        Some(duration) => Ok(duration),
    }
}

pub(crate) fn io_err_timeout(error: String) -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, error)
}

impl fmt::Debug for Stream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self {
            Stream::Http(tcpstream) => write!(f, "{:?}", tcpstream),
            #[cfg(feature = "tls")]
            Stream::Https(tlsstream) => write!(f, "{:?}", tlsstream.get_ref()),
            Stream::Test(_, _) => write!(f, "Stream(Test)"),
        }
    }
}

impl Stream {
    fn logged_create(stream: Stream) -> Stream {
        debug!("created stream: {:?}", stream);
        stream
    }

    pub(crate) fn from_vec(v: Vec<u8>) -> Stream {
        Stream::logged_create(
            Stream::Test(Box::new(Cursor::new(v)), vec![]),
        )
    }

    fn from_tcp_stream(t: TcpStream) -> Stream {
        Stream::logged_create(
            Stream::Http(t),
        )
    }

    #[cfg(feature = "tls")]
    fn from_tls_stream(t: StreamOwned<ClientConnection, TcpStream>) -> Stream {
        Stream::logged_create(
            Stream::Https(t),
        )
    }

    pub(crate) fn socket(&self) -> Option<&TcpStream> {
        match &self {
            Stream::Http(b) => Some(&b),
            #[cfg(feature = "tls")]
            Stream::Https(b) => Some(b.get_ref()),
            _ => None,
        }
    }

    pub(crate) fn set_read_timeout(&self, timeout: Duration) -> io::Result<()> {
        if let Some(socket) = self.socket() {
            socket.set_read_timeout(Some(timeout))
        } else {
            Ok(())
        }
    }

    #[cfg(test)]
    pub fn to_write_vec(&self) -> Vec<u8> {
        match &self.get_ref() {
            Stream::Test(_, writer) => writer.clone(),
            _ => panic!("to_write_vec on non Test stream"),
        }
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Stream::Http(sock) => sock.read(buf),
            #[cfg(feature = "tls")]
            Stream::Https(stream) => read_https(stream, buf),
            Stream::Test(reader, _) => reader.read(buf),
        }
    }
}

impl<R: Read> From<ChunkDecoder<R>> for Stream
where
    R: Read,
    Stream: From<R>,
{
    fn from(chunk_decoder: ChunkDecoder<R>) -> Stream {
        chunk_decoder.into_inner().into()
    }
}

#[cfg(feature = "tls")]
fn read_https(
    stream: &mut StreamOwned<ClientConnection, TcpStream>,
    buf: &mut [u8],
) -> io::Result<usize> {
    match stream.read(buf) {
        Ok(size) => Ok(size),
        Err(ref e) if is_close_notify(e) => Ok(0),
        Err(e) => Err(e),
    }
}

#[allow(deprecated)]
#[cfg(feature = "tls")]
fn is_close_notify(e: &std::io::Error) -> bool {
    if e.kind() != io::ErrorKind::ConnectionAborted {
        return false;
    }

    if let Some(msg) = e.get_ref() {
        // :(

        return msg.description().contains("CloseNotify");
    }

    false
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Stream::Http(sock) => sock.write(buf),
            #[cfg(feature = "tls")]
            Stream::Https(stream) => stream.write(buf),
            Stream::Test(_, writer) => writer.write(buf),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            Stream::Http(sock) => sock.flush(),
            #[cfg(feature = "tls")]
            Stream::Https(stream) => stream.flush(),
            Stream::Test(_, writer) => writer.flush(),
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        debug!("dropping stream: {:?}", self);
    }
}

pub(crate) fn connect_http(unit: &Unit, hostname: &str) -> Result<Stream, Error> {
    //
    let port = unit.url.port().unwrap_or(80);

    connect_host(unit, hostname, port).map(Stream::from_tcp_stream)
}
#[cfg(feature = "tls")]
use once_cell::sync::Lazy;
#[cfg(feature = "tls")]
use std::{convert::TryFrom, sync::Arc};
#[cfg(feature = "tls")]
static TLS_CONF: Lazy<Arc<rustls::ClientConfig>> = Lazy::new(|| {
    let mut root_store = rustls::RootCertStore::empty();
    #[cfg(not(feature = "native-tls"))]
    root_store.add_server_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.0.iter().map(|ta| {
        rustls::OwnedTrustAnchor::from_subject_spki_name_constraints(
            ta.subject,
            ta.spki,
            ta.name_constraints,
        )
    }));
    #[cfg(feature = "native-tls")]
    root_store.add_server_trust_anchors(
        rustls_native_certs::load_native_certs().expect("Could not load platform certs"),
    );

    let config = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    Arc::new(config)
});

#[cfg(feature = "tls")]
pub(crate) fn connect_https(unit: &Unit, hostname: &str) -> Result<Stream, Error> {
    let port = unit.url.port().unwrap_or(443);

    let tls_conf: Arc<rustls::ClientConfig> = unit
        .agent
        .config
        .tls_config
        .as_ref()
        .map(|c| c.0.clone())
        .unwrap_or_else(|| TLS_CONF.clone());
    let mut sock = connect_host(unit, hostname, port)?;
    let mut sess = rustls::ClientConnection::new(
        tls_conf,
        rustls::ServerName::try_from(hostname).map_err(|_e| ErrorKind::Dns.new())?,
    )
    .map_err(|e| ErrorKind::Io.new().src(e))?;
    // TODO rustls 0.20.1: Add src to ServerName error (0.20 didn't implement StdError trait for it)

    sess.complete_io(&mut sock)
        .map_err(|err| ErrorKind::ConnectionFailed.new().src(err))?;
    let stream = rustls::StreamOwned::new(sess, sock);

    Ok(Stream::from_tls_stream(stream))
}

pub(crate) fn connect_host(unit: &Unit, hostname: &str, port: u16) -> Result<TcpStream, Error> {
    let netloc = format!("{}:{}", hostname, port);

    // TODO: Find a way to apply deadline to DNS lookup.
    let sock_addrs = netloc.to_socket_addrs()?;

    let mut any_err = None;
    let mut any_stream = None;
    // Find the first sock_addr that accepts a connection
    for sock_addr in sock_addrs {
        // ensure connect timeout or overall timeout aren't yet hit.
        let timeout = time_until_deadline(unit.deadline)?;
        debug!("connecting to {} at {}", netloc, &sock_addr);

        let stream = TcpStream::connect_timeout(&sock_addr, timeout);

        if let Ok(stream) = stream {
            any_stream = Some(stream);
            break;
        } else if let Err(err) = stream {
            any_err = Some(err);
        }
    }

    let stream = if let Some(stream) = any_stream {
        stream
    } else if let Some(e) = any_err {
        return Err(ErrorKind::ConnectionFailed.msg("Connect error").src(e));
    } else {
        return Err(ErrorKind::Dns.msg(&format!("No ip address for {}", hostname)));
        //panic!("shouldn't happen: failed to connect to all IPs, but no error");
    };

    let deadline = time_until_deadline(unit.deadline)?;
    stream.set_read_timeout(Some(deadline))?;
    stream.set_write_timeout(Some(deadline))?;

    Ok(stream)
}

#[cfg(test)]
pub(crate) fn connect_test(unit: &Unit) -> Result<Stream, Error> {
    use crate::test;
    test::resolve_handler(unit)
}

#[cfg(not(test))]
pub(crate) fn connect_test(unit: &Unit) -> Result<Stream, Error> {
    Err(ErrorKind::UnknownScheme.msg(&format!("unknown scheme '{}'", unit.url.scheme())))
}

#[cfg(not(feature = "tls"))]
pub(crate) fn connect_https(unit: &Unit, _hostname: &str) -> Result<Stream, Error> {
    Err(ErrorKind::UnknownScheme
        .msg("URL has 'https:' scheme but ureq was build without HTTP support")
        .url(unit.url.clone()))
}
