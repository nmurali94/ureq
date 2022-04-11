use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::net::{SocketAddr};

use io_uring::{connect, dns};

use crate::error::Error;
#[cfg(feature = "tls")]
use crate::Agent;

use crate::error::ErrorKind;

pub enum Stream {
    Http(TcpStream),
    #[cfg(feature = "tls")]
    Https(rustls::StreamOwned<rustls::ClientConnection, TcpStream>),
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Stream::Http(sock) => sock.read(buf),
            #[cfg(feature = "tls")]
            Stream::Https(stream) => match stream.read(buf) {
                Err(ref e) if is_close_notify(e) => Ok(0),
                v => v,
            },
        }
    }
}

#[allow(deprecated)]
#[cfg(feature = "tls")]
fn is_close_notify(e: &std::io::Error) -> bool {
    if e.kind() != io::ErrorKind::ConnectionAborted {
        return false;
    }
    if let Some(msg) = e.get_ref() {
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
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            Stream::Http(sock) => sock.flush(),
            #[cfg(feature = "tls")]
            Stream::Https(stream) => stream.flush(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct HostAddr<'a> {
    pub host: &'a str,
    pub port: u16,
}

pub(crate) fn connect_http_v2<'a>(
    urls: impl Iterator<Item = HostAddr<'a>>,
) -> Result<(Vec<String>, Vec<TcpStream>), Error> {

    let (names, ports): (Vec<_>, Vec<_>) =
        urls.map(|u| (u.host, u.port)).unzip();

    let msgs = dns(names.into_iter())?;

    let (names, socks): (Vec<_>, Vec<_>) = msgs.into_iter().zip(ports).map(|(msg, port)| {
        let (name, ips) = msg;
        let ipaddr = ips[0];
        (name, SocketAddr::new(ipaddr, port))
    }).unzip();

    match connect(socks) {
        Ok(v) => Ok((names, v)),
        Err(e) => Err(Error::from(e)),
    }
}

#[cfg(feature = "tls")]
use std::{convert::TryFrom, sync::Arc};

#[cfg(feature = "tls")]
pub(crate) fn connect_https_v2(
    mut sock: TcpStream,
    hostname: &str,
    agent: &Agent,
) -> Result<Stream, Error> {
    let tls_conf: Arc<rustls::ClientConfig> = agent.tls_config.clone();
    let mut sess = rustls::ClientConnection::new(
        tls_conf,
        rustls::ServerName::try_from(hostname).map_err(|_e| ErrorKind::Dns.new())?,
    )
    .map_err(|e| ErrorKind::Io.new().src(e))?;
    // TODO rustls 0.20.1: Add src to ServerName error (0.20 didn't implement StdError trait for it)

    sess.complete_io(&mut sock)
        .map_err(|err| ErrorKind::ConnectionFailed.new().src(err))?;
    let stream = rustls::StreamOwned::new(sess, sock);

    Ok(Stream::Https(stream))
}

