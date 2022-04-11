use std::io::{self, Write};

use crate::url::{Scheme, Url};

use crate::error::Error;
use crate::stream::{self, Stream, HostAddr};
use crate::Agent;

/// Send request line + headers (all up until the body).
pub(crate) fn send_request(host: &str, path: &str, user_agent: &str, stream: &mut Stream) -> io::Result<()> {
    // request line
    let mut v = arrayvec::ArrayVec::<u8, 512>::new_const();

    let _ = v.try_extend_from_slice(b"GET ");
    let _ = v.try_extend_from_slice(path.as_bytes());
    let _ = v.try_extend_from_slice(b" HTTP/1.1\r\n");

    // host header if not set by user.
    let _ = v.try_extend_from_slice(b"Host: ");
    let _ = v.try_extend_from_slice(host.as_bytes());
    let _ = v.try_extend_from_slice(b"\r\n");

    let _ = v.try_extend_from_slice(b"User-Agent: ");
    let _ = v.try_extend_from_slice(user_agent.as_bytes());
    let _ = v.try_extend_from_slice(b"\r\n");

    // finish

    let _ = v.try_extend_from_slice(b"\r\n");

    stream.write_all(&v)?;

    Ok(())
}

#[cfg(not(feature = "tls"))]
pub(crate) fn connect_v2(
    _agent: &Agent,
    urls: impl Iterator<Item = Url>,
) -> Result<Iterator<Item = Stream>, Error> {
    stream::connect_http_v2(urls)
        .map(|s| s.into_iter().map(Stream::Http).into())
}

#[cfg(feature = "tls")]
pub(crate) fn connect_v2(agent: &Agent, urls: &[Url]) -> Result<Vec<Stream>, Error> {
    let (hostaddrs, schemes): (Vec<_>, Vec<_>) = urls.iter().map(|u| (HostAddr { host: u.host_str(), port: u.port() }, u.scheme())).unzip();
        println!("HERE {:?}", hostaddrs);
    let (names, streams) = stream::connect_http_v2(hostaddrs.into_iter())?;
    let mut ss = Vec::with_capacity(streams.len());
    for (i, (stream, scheme)) in streams.into_iter().zip(schemes.iter()).enumerate() {
        let s = match scheme {
            Scheme::Http => Stream::Http(stream),
            Scheme::Https => stream::connect_https_v2(stream, &names[i], agent)?,
        };
        ss.push(s);
    }
    Ok(ss)
}
