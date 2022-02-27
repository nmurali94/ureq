use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::{fmt};
use std::net::{UdpSocket, SocketAddr};

use dns_parser::RData::A;
use dns_parser::{Builder, Packet, QueryClass, QueryType};

use io_uring::{dns};

use chunked_transfer::Decoder as ChunkDecoder;
use crate::url::Url;

#[cfg(feature = "tls")]
use rustls::ClientConnection;
#[cfg(feature = "tls")]
use rustls::StreamOwned;

use crate::{error::Error};
use crate::Agent;

use crate::error::ErrorKind;
use crate::unit::{Unit};

#[allow(clippy::large_enum_variant)]
pub enum Stream {
    Http(TcpStream),
    #[cfg(feature = "tls")]
    Https(rustls::StreamOwned<rustls::ClientConnection, TcpStream>),
}

impl fmt::Debug for Stream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self {
            Stream::Http(tcpstream) => write!(f, "{:?}", tcpstream),
            #[cfg(feature = "tls")]
            Stream::Https(tlsstream) => write!(f, "{:?}", tlsstream.get_ref()),
        }
    }
}

impl Stream {
    fn logged_create(stream: Stream) -> Stream {
        stream
    }

    pub fn from_tcp_stream(t: TcpStream) -> Stream {
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
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Stream::Http(sock) => sock.read(buf),
            #[cfg(feature = "tls")]
            Stream::Https(stream) => read_https(stream, buf),
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

pub(crate) fn connect_http(unit: &Unit) -> Result<Stream, Error> {
    connect_host(unit).map(Stream::from_tcp_stream)
}
pub(crate) fn connect_http_v2(urls: &[Url], ports: &[u16]) -> Result<Vec<TcpStream>, Error> {
    let urls: Vec<_> = urls.iter().map(|u| u.host_str()).collect();
    match connect_hosts(urls.as_slice(), ports) {
		Ok(v) => Ok(v),
		Err(e) => Err(Error::from(e)),
	}
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
pub(crate) fn connect_https_v2(mut sock: TcpStream, hostname: &str, agent: &Agent) -> Result<Stream, Error> {

    let tls_conf: Arc<rustls::ClientConfig> = agent
        .config
        .tls_config
        .as_ref()
        .map(|c| c.0.clone())
        .unwrap_or_else(|| TLS_CONF.clone());
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


#[cfg(feature = "tls")]
pub(crate) fn connect_https(unit: &Unit) -> Result<Stream, Error> {
    let mut sock = connect_host(unit)?;
    let hostname = unit
        .url
        .host_str()
        ;

    let tls_conf: Arc<rustls::ClientConfig> = unit
        .agent
        .config
        .tls_config
        .as_ref()
        .map(|c| c.0.clone())
        .unwrap_or_else(|| TLS_CONF.clone());
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


fn to_socket_addrs(netloc: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
    let mut dmsg = Builder::new_query(13, true);
    dmsg.add_question(netloc, false, QueryType::A, QueryClass::IN);
    let dmsg = dmsg.build().expect("Bad DNS Query");

    let socket = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind to socket");

    // Receives a single datagram message on the socket. If `buf` is too small to hold
    // the message, it will be cut off.
    let mut buf = [0; 512];
    let _ = socket.send_to(&dmsg, "127.0.0.1:53").expect("Failed to send to socket");

    let (amt, _sock) = socket.recv_from(&mut buf).expect("Failed to recv frmo socket");
    let msg = Packet::parse(&buf[..amt]).expect("Bad DNS response");
    //println!("Answer - {:?} ", msg);
    //std::process::exit(-1);
    let socks = msg.answers.iter().filter_map(|ans| {
        match ans.data {
            A(ipv4) => {
                let addr = ipv4.0;
                Some(SocketAddr::new(std::net::IpAddr::V4(addr), port))
            },
            _ => None,
        }
    })
    .collect();
    Ok(socks)
	/*
	use std::net::ToSocketAddrs;
	let addrs = (netloc, port).to_socket_addrs()?;
	Ok(addrs.collect())
	*/
}

fn connect_hosts(names: &[&str], ports: &[u16]) -> Result<Vec<TcpStream>, io::Error> {
    //let names: Vec<_> = configs.split(|&b| b == b'\n').filter_map(|s| std::str::from_utf8(s).ok()).map(|s| s.trim()).collect();
    let mut buffers = vec![[0; 512]; names.len()];
    let msgs = dns(names, &mut buffers).expect("Failed to resolve dns");
	let mut socks = Vec::new();
	for (msg, port) in msgs.iter().zip(ports.iter()) {
		let (_name, ips) = msg.get().expect("Failed to parse packet");
		let ipaddr  = ips[0];
		let socketaddr = SocketAddr::new(ipaddr, *port);
		
		socks.push(socketaddr);
	}
	println!("Sockets {}", socks.len());
	io_uring::connect(socks)
}

fn connect_host(unit: &Unit) -> Result<TcpStream, Error> {
    //println!("Netloc {:?}", netloc);
    let hostname = unit
        .url
        .host_str()
        ;
    let port = unit.url.port();

    // TODO: Find a way to apply deadline to DNS lookup.
    let sock_addrs = to_socket_addrs(hostname, port)?;
    //let sock_addrs = netloc.to_socket_addrs()?;

    let mut any_err = None;
    let mut any_stream = None;
    // Find the first sock_addr that accepts a connection
    for sock_addr in sock_addrs {
        // ensure connect timeout or overall timeout aren't yet hit.
        //println!("connecting to {:?} at {}", netloc, &sock_addr);

        // connect_timeout uses non-blocking connect which runs a large number of poll syscalls
        //let stream = TcpStream::connect_timeout(&sock_addr, timeout);
        let stream = TcpStream::connect(sock_addr);
        // Debug format
        //println!("Connect time: {:?}", elapsed);

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

    stream.set_nodelay(true)?;

    Ok(stream)
}

