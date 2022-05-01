use dns_parser::RData::A;
use dns_parser::{Builder, Packet, QueryClass, QueryType};
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream, UdpSocket};

#[cfg(feature = "tls")]
use crate::agent::Agent;
use crate::error::Error;

#[cfg(feature = "tls")]
use crate::error::ErrorKind;

type IpAddrs = Vec<IpAddr>;

pub enum Stream {
    Http(TcpStream),
    #[cfg(feature = "tls")]
    Https(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
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

pub(crate) fn connect_http(url: HostAddr) -> Result<(String, TcpStream), Error> {
    let host = url.host;
    let port = url.port;

    let (name, ips) = dns(host)?;

    let ipaddr = ips[0];
    let socket = SocketAddr::new(ipaddr, port);

    match connect_inner(socket) {
        Ok(v) => Ok((name, v)),
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

    Ok(Stream::Https(Box::new(stream)))
}

pub fn dns(name: &str) -> io::Result<(String, IpAddrs)> {
    let base = std::net::SocketAddr::from(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 0));
    let socket = UdpSocket::bind(base)?;
    let addr = std::net::SocketAddr::from(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 53), 53));

    let mut dmsg = Builder::new_query(13 as _, true);
    dmsg.add_question(name, false, QueryType::A, QueryClass::IN);
    let dmsg = dmsg.build().expect("Bad DNS Query");

    let c = socket.send_to(&dmsg, &addr)?;
    assert!(c == dmsg.len(), "Incomplete dns message");
    let mut buf = [0; 512];
    let (amt, _) = socket.recv_from(&mut buf[..])?;
    let buf = &buf[..amt];
    let packet = Packet::parse(buf).expect("Failed to parse dns packet");
    let q = packet
        .questions
        .first()
        .expect("Question should never be empty");
    let socks = packet
        .answers
        .iter()
        .filter_map(|ans| match ans.data {
            A(ipv4) => {
                let addr = ipv4.0;
                Some(std::net::IpAddr::V4(addr))
            }
            _ => None,
        })
        .collect();
    Ok((q.qname.to_string(), socks))
}

fn connect_inner(socket: SocketAddr) -> io::Result<TcpStream> {
    let tcp = TcpStream::connect(socket)?;
    tcp.set_nodelay(true)?;
    Ok(tcp)
}
