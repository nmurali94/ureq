use std::io::{Result as IoResult, Write};

#[cfg(feature = "tls")]
use crate::url::Scheme;
use crate::url::Url;

use crate::agent::Agent;
use crate::error::Error;
#[cfg(feature = "tls")]
use crate::stream::connect_https_v2;
use crate::stream::{connect_http, HostAddr, Stream};

/// Send request line + headers (all up until the body).
pub(crate) fn send_request(
    host: &str,
    path: &str,
    user_agent: &str,
    stream: &mut Stream,
) -> IoResult<()> {
    // request line
    let mut buf = [0; 512];
    let mut v = &mut buf[..];

    let _ = v.write(b"GET ");
    let _ = v.write(path.as_bytes());
    let _ = v.write(b" HTTP/1.1\r\n");

    // host header if not set by user.
    let _ = v.write(b"Host: ");
    let _ = v.write(host.as_bytes());
    let _ = v.write(b"\r\n");

    let _ = v.write(b"User-Agent: ");
    let _ = v.write(user_agent.as_bytes());
    let _ = v.write(b"\r\n");

    // finish

    let _ = v.write(b"\r\n");
    let rem = v.len();

    stream.write_all(&buf[..(512-rem)])
}

#[cfg(not(feature = "tls"))]
pub(crate) fn connect(_agent: &Agent, url: &Url) -> Result<Stream, Error> {
    let h = HostAddr {
        host: url.host_str(),
        port: url.port(),
    };
    let (_, s) = connect_http(h)?;
    Ok(Stream::Http(s))
}

#[cfg(feature = "tls")]
pub(crate) fn connect(agent: &Agent, url: &Url) -> Result<Stream, Error> {
    let h = HostAddr {
        host: url.host_str(),
        port: url.port(),
    };
    let (name, stream) = connect_http(h)?;
    let s = match url.scheme() {
        Scheme::Http => Stream::Http(stream),
        Scheme::Https => connect_https_v2(stream, &name, agent)?,
    };
    Ok(s)
}
