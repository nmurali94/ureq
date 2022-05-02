use crate::url::Url;

use crate::response::{Response};
use crate::unit::{connect, send_request};
use crate::agent::Agent;
use crate::error::{Error};

/// Request instances are builders that creates a request.
pub struct Request;

impl Request {
    pub fn call(agent: &Agent, url: &Url) -> Result<Response, Error> {
        connect(agent, url)
            .and_then(|mut stream| {
                send_request(url.host_str(), url.path(), agent.user_agent, &mut stream)
                    .map(|_| stream)
                    .map_err(|e| e.into())
            })
            .and_then(Response::do_from_stream)
    }
}
