use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::net::{SocketAddr, UdpSocket};

use dns_parser::RData::A;
use dns_parser::{Builder, Packet, QueryClass, QueryType};

use io_uring::{connect, dns};

use crate::url::Url;
use chunked_transfer::Decoder as ChunkDecoder;

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

impl<R: Read> From<ChunkDecoder<R>> for Stream
where
    Stream: From<R>,
{
    fn from(chunk_decoder: ChunkDecoder<R>) -> Stream {
        chunk_decoder.into_inner().into()
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

pub(crate) fn connect_http(url: &Url) -> Result<Stream, Error> {
    let hostname = url.host_str();
    let port = url.port();
    connect_host(hostname, port).map(Stream::Http)
}
pub(crate) fn connect_http_v2<'a>(
    urls: impl IntoIterator<Item = &'a Url>,
) -> Result<Vec<TcpStream>, Error> {
    let (names, ports): (Vec<_>, Vec<_>) =
        urls.into_iter().map(|u| (u.host_str(), u.port())).unzip();
    let msgs = dns(names.into_iter())?;
    let socks = msgs.iter().zip(ports).map(|(msg, port)| {
        let (_name, ips) = msg;
        let ipaddr = ips[0];
        SocketAddr::new(ipaddr, port)
    });
    match connect(socks) {
        Ok(v) => Ok(v),
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

#[cfg(feature = "tls")]
pub(crate) fn connect_https(url: &Url, agent: &Agent) -> Result<Stream, Error> {
    let hostname = url.host_str();
    let port = url.port();
    let mut sock = connect_host(hostname, port)?;

    let server_name = rustls::ServerName::try_from(hostname).map_err(|_e| ErrorKind::Dns.new())?;
    let mut sess = rustls::ClientConnection::new(agent.tls_config.clone(), server_name)
        .map_err(|e| ErrorKind::Io.new().src(e))?;
    // TODO rustls 0.20.1: Add src to ServerName error (0.20 didn't implement StdError trait for it)

    sess.complete_io(&mut sock)
        .map_err(|err| ErrorKind::ConnectionFailed.new().src(err))?;
    let stream = rustls::StreamOwned::new(sess, sock);

    Ok(Stream::Https(stream))
}

fn to_socket_addrs(netloc: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
    let mut dmsg = Builder::new_query(13, true);
    dmsg.add_question(netloc, false, QueryType::A, QueryClass::IN);
    let dmsg = dmsg.build().expect("Bad DNS Query");

    let socket = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind to socket");

    // Receives a single datagram message on the socket. If `buf` is too small to hold
    // the message, it will be cut off.
    let mut buf = [0; 512];
    let _ = socket.send_to(&dmsg, "127.0.0.1:53")?;

    let (amt, _sock) = socket
        .recv_from(&mut buf)
        .expect("Failed to recv frmo socket");
    let msg = Packet::parse(&buf[..amt]).expect("Bad DNS response");
    let socks = msg
        .answers
        .iter()
        .filter_map(|ans| match ans.data {
            A(ipv4) => {
                let addr = ipv4.0;
                Some(SocketAddr::new(std::net::IpAddr::V4(addr), port))
            }
            _ => None,
        })
        .collect();
    Ok(socks)
}

fn connect_host(hostname: &str, port: u16) -> Result<TcpStream, Error> {
    // TODO: Find a way to apply deadline to DNS lookup.
    let sock_addrs = to_socket_addrs(hostname, port)?;
    //let sock_addrs = netloc.to_socket_addrs()?;

    let mut any_err = None;
    let mut any_stream = None;
    // Find the first sock_addr that accepts a connection
    for sock_addr in sock_addrs {
        // connect_timeout uses non-blocking connect which runs a large number of poll syscalls
        //let stream = TcpStream::connect_timeout(&sock_addr, timeout);
        let stream = TcpStream::connect(sock_addr);

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
    };

    stream.set_nodelay(true)?;

    Ok(stream)
}
