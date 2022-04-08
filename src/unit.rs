use std::io::{self, Write};

use crate::url::{Scheme, Url};

use crate::error::{Error, ErrorKind};
use crate::response::Response;
use crate::stream::{self, Stream};
use crate::Agent;

/// A Unit is fully-built Request, ready to execute.
///
/// *Internal API*
pub(crate) struct Unit {
    pub agent: Agent,
    pub url: Url,
}

#[derive(Clone, Copy)]
pub enum Status {
    Success = 200,
    BadRequest = 400,
    NotFound = 404,
    InternalServerError = 500,
    Unsupported,
}

impl From<u16> for Status {
    fn from(n: u16) -> Self {
        use Status::*;
        match n {
            200 => Success,
            400 => BadRequest,
            404 => NotFound,
            500 => InternalServerError,
            _ => Unsupported,
        }
    }
}

impl Status {
    pub fn to_str(self) -> &'static str {
        use Status::*;
        match self {
            Success => "Ok",
            BadRequest => "Bad Request",
            NotFound => "Not Found",
            InternalServerError => "Internal Server Error",
            Unsupported => "Unknown",
        }
    }
}

impl Unit {
    pub(crate) fn new(agent: Agent, url: Url) -> Self {
        Unit { agent, url }
    }

    /// Perform a connection. Follows redirects.
    pub(crate) fn connect(&self) -> Result<Response, Error> {
        let resp = self.connect_inner()?;

        let (_version, status) = resp.get_status_line()?;
        // handle redirects
        match status {
            Status::Success => Ok(resp),
            _ => Err(ErrorKind::TooManyRedirects.new()),
        }
    }

    /// Perform a connection. Does not follow redirects.
    fn connect_inner(&self) -> Result<Response, Error> {
        // open socket
        let mut stream = self.connect_socket()?;

        let send_result = send_request(&self.url, &self.agent, &mut stream);

        if let Err(err) = send_result {
            // not a pooled connection, propagate the error.
            Err(err.into())
        } else {
            // start reading the response to process cookies and redirects.
            Response::do_from_stream(stream)
        }
    }

    /// Connect the socket, either by using the pool or grab a new one.
    fn connect_socket(&self) -> Result<Stream, Error> {
        match self.url.scheme() {
            Scheme::Http => stream::connect_http(self),
            #[cfg(feature = "tls")]
            Scheme::Https => stream::connect_https(self),
        }
    }
}

/// Send request line + headers (all up until the body).
pub(crate) fn send_request(url: &Url, agent: &Agent, stream: &mut Stream) -> io::Result<()> {
    // request line
    let mut v = arrayvec::ArrayVec::<u8, 512>::new_const();

    let _ = v.try_extend_from_slice(b"GET ");
    let _ = v.try_extend_from_slice(url.path().as_bytes());
    let _ = v.try_extend_from_slice(b" HTTP/1.1\r\n");

    // host header if not set by user.
    let _ = v.try_extend_from_slice(b"Host: ");
    let _ = v.try_extend_from_slice(url.host_str().as_bytes());
    let _ = v.try_extend_from_slice(b"\r\n");

    let _ = v.try_extend_from_slice(b"User-Agent: ");
    let _ = v.try_extend_from_slice(agent.user_agent.as_bytes());
    let _ = v.try_extend_from_slice(b"\r\n");

    // finish

    let _ = v.try_extend_from_slice(b"\r\n");

    stream.write_all(&v)?;

    Ok(())
}
#[cfg(not(feature = "tls"))]
pub(crate) fn connect_v2(_agent: &Agent, urls: &[Url]) -> Result<Vec<Stream>, Error> {
    let p: Vec<_> = urls.iter().map(|_| 80).collect();
    let streams = stream::connect_http_v2(urls, p.as_slice())?;
    let ss = streams
        .into_iter()
        .map(|stream| Stream::from_tcp_stream(stream))
        .collect();
    Ok(ss)
}

#[cfg(feature = "tls")]
pub(crate) fn connect_v2(agent: &Agent, urls: &[Url]) -> Result<Vec<Stream>, Error> {
    let p: Vec<_> = urls.iter().map(|u| u.port()).collect();
    let streams = stream::connect_http_v2(urls, p.as_slice())?;
    let mut ss = Vec::new();
    for (i, (stream, url)) in streams.into_iter().zip(urls.iter()).enumerate() {
        let s = match url.scheme() {
            Scheme::Http => Stream::from_tcp_stream(stream),
            Scheme::Https => stream::connect_https_v2(stream, urls[i].host_str(), agent)?,
        };
        ss.push(s);
    }
    Ok(ss)
}
