use crate::url::Url;

use crate::response::Status;
use crate::unit::{connect, send_request};
use crate::Response;
use crate::{agent::Agent, error::Error, error::ErrorKind};

pub type Result<T> = std::result::Result<T, Error>;

/// Request instances are builders that creates a request.
pub struct Request;

impl Request {
    /// Sends the request with no body and blocks the caller until done.
    ///
    /// Use this with GET, HEAD, OPTIONS or TRACE. It sends neither
    /// Content-Length nor Transfer-Encoding.
    ///

    pub fn call(agent: Agent, url: Url) -> Result<Response> {
        connect(&agent, &url)
            .and_then(|mut stream| {
                send_request(url.host_str(), url.path(), &agent.user_agent, &mut stream)
                    .map(|_| stream)
                    .map_err(|e| e.into())
            })
            .and_then(Response::do_from_stream)
            .and_then(|resp| {
                resp.get_status_line().and_then(|(_,status)|
                // handle redirects
                match status {
                    Status::Success => Ok(resp),
                    _ => Err(ErrorKind::TooManyRedirects.new()),
                })
            })
    }
}
