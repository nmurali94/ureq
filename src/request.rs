use crate::url::Url;

use crate::response::Status;
#[cfg(feature = "tls")]
use crate::stream::connect_https;
use crate::stream::{connect_http, Stream};
use crate::unit::{connect_v2, send_request};
use crate::url::Scheme;
use crate::Response;
use crate::{agent::Agent, error::Error, error::ErrorKind};

pub type Result<T> = std::result::Result<T, Error>;

/// Request instances are builders that creates a request.
pub struct Request {
    agent: Agent,
    url: Url,
}

pub(crate) fn call_urls(agent: Agent, urls: Vec<Url>) -> Result<Vec<Stream>> {
    let mut streams = connect_v2(&agent, urls.as_slice())?;
    for (url, stream) in urls.iter().zip(streams.iter_mut()) {
        send_request(url, &agent, stream)?;
    }
    Ok(streams)
}

impl Request {
    pub(crate) fn new(agent: Agent, url: &str) -> Result<Request> {
        let url = Url::parse(url).map_err(|_e| ErrorKind::HTTP.new())?;
        Ok(Request { agent, url })
    }

    /// Sends the request with no body and blocks the caller until done.
    ///
    /// Use this with GET, HEAD, OPTIONS or TRACE. It sends neither
    /// Content-Length nor Transfer-Encoding.
    ///

    pub fn call(self) -> Result<Response> {
        let mut stream = match self.url.scheme() {
            Scheme::Http => connect_http(&self.url),
            #[cfg(feature = "tls")]
            Scheme::Https => connect_https(&self.url, &self.agent),
        }?;

        send_request(&self.url, &self.agent, &mut stream)?;

        // start reading the response to process headers
        let resp = Response::do_from_stream(stream)?;

        let (_version, status) = resp.get_status_line()?;
        // handle redirects
        match status {
            Status::Success => Ok(resp),
            _ => Err(ErrorKind::TooManyRedirects.new()),
        }
    }
}
