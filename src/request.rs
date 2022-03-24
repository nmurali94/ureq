use crate::url::{Url};

use crate::unit::{Unit, connect_v2, send_request};
use crate::stream::{Stream};
use crate::Response;
use crate::{agent::Agent, error::Error, error::ErrorKind};

pub type Result<T> = std::result::Result<T, Error>;

/// Request instances are builders that creates a request.
pub struct Request {
    agent: Agent,
    url: Url,
}

pub(crate) fn call_urls(agent: Agent, urls: Vec<String>) -> Result<Vec<Stream>> {
    let urls: Vec<_> = urls.into_iter()
        .filter_map(|url| Url::parse(url).map_err(|_e| ErrorKind::HTTP.new()).ok())
        .collect();

    let mut streams = connect_v2(&agent, urls.as_slice())?;
    for (url, stream) in urls.iter().zip(streams.iter_mut()) {
        send_request(url, &agent, stream)?;
    }
    Ok(streams)
}

impl Request {
    pub(crate) fn new(agent: Agent, url: &str) -> Result<Request> {
        let url = Url::parse(url.to_owned()).map_err(|_e| ErrorKind::HTTP.new())?;
        Ok(Request {
            agent,
            url,
        })
    }

    /// Sends the request with no body and blocks the caller until done.
    ///
    /// Use this with GET, HEAD, OPTIONS or TRACE. It sends neither
    /// Content-Length nor Transfer-Encoding.
    ///

    pub fn call(self) -> Result<Response> {
        let unit = Unit::new(
            self.agent,
            self.url,
        );
        let response = unit.connect()?; //.map_err(|e| e.url(self.url))?;

        let (_version, status, _text) = response.get_status_line()?;

        if status >= 400 {
            Err(Error::Status(status, response))
        } else {
            Ok(response)
        }
    }
}

