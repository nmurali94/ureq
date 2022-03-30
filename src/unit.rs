use std::io::{self, Write};

use crate::url::Url;

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

impl Unit {
    pub(crate) fn new(agent: Agent, url: Url) -> Self {
        Unit {
            agent,
            url,
        }
    }

    /// Perform a connection. Follows redirects.
    pub(crate) fn connect(&self) -> Result<Response, Error> {
        let resp = self.connect_inner()?;

        let (_version, status, _text) = resp.get_status_line()?;
        // handle redirects
        if (300..399).contains(&status) {
            Err(ErrorKind::TooManyRedirects.new())
        } else {
            Ok(resp)
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
            "http" | "https" => (),
            scheme => return Err(ErrorKind::UnknownScheme.msg(&format!("unknown scheme '{}'", scheme))),
        };
        let stream = match self.url.scheme() {
            "http" => stream::connect_http(self),
            "https" => stream::connect_https(self),
            scheme => Err(ErrorKind::UnknownScheme.msg(&format!("unknown scheme {}", scheme))),
        }?;
        Ok(stream)
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

pub(crate) fn connect_v2(agent: &Agent, urls: &[Url]) -> Result<Vec<Stream>, Error> {
    let p: Vec<_> = urls.iter().map(|u| if u.scheme() == "https" { 443 } else { 80 }).collect();
    let streams = stream::connect_http_v2(urls, p.as_slice())?;
    let mut ss = Vec::new();
    for (i, (stream, url)) in streams.into_iter().zip(urls.iter()).enumerate() {
        let s = if url.scheme() == "https" {
            stream::connect_https_v2(stream, urls[i].host_str(), agent)?
        } else {
            Stream::from_tcp_stream(stream)
        };
        ss.push(s);
    }
    Ok(ss)
}
