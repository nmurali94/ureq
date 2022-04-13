use crate::url::Url;

use crate::response::Status;
use crate::unit::{connect, send_request};
use crate::Response;
use crate::{agent::Agent, error::Error, error::ErrorKind};

pub type Result<T> = std::result::Result<T, Error>;

/// Request instances are builders that creates a request.
pub struct Request { }

impl Request {

    /// Sends the request with no body and blocks the caller until done.
    ///
    /// Use this with GET, HEAD, OPTIONS or TRACE. It sends neither
    /// Content-Length nor Transfer-Encoding.
    ///

    pub fn call(agent: Agent, url: Url) -> Result<Response> {
        let mut stream = connect(&agent, &url)?;

        send_request(url.host_str(), url.path(), &agent.user_agent, &mut stream)?;

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
