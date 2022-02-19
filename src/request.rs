use std::{fmt};

use crate::url::{Url};

use crate::unit::{self, Unit};
use crate::Response;
use crate::{agent::Agent, error::Error, error::ErrorKind};

pub type Result<T> = std::result::Result<T, Error>;

/// Request instances are builders that creates a request.
///
#[derive(Clone)]
pub struct Request {
    agent: Agent,
    method: String,
    url: Url,
}
pub struct GetRequests {
    agent: Agent,
    urls: Vec<Url>,
}

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Request({} {:?})",
            self.method, self.url,
        )
    }
}

impl GetRequests {
    pub(crate) fn new(agent: Agent, urls: Vec<String>) -> Result<GetRequests> {
        let urls = urls.into_iter()
            .filter_map(|url| Url::parse(url).map_err(|_e| ErrorKind::HTTP.new()).ok())
            .collect();
        Ok(GetRequests {
            agent,
            urls,
        })
    }
    /*

    /// Sends the request with no body and blocks the caller until done.
    ///
    /// Use this with GET, HEAD, OPTIONS or TRACE. It sends neither
    /// Content-Length nor Transfer-Encoding.
    ///
    /// ```
    /// # fn main() -> Result<(), ureq::Error> {
    /// # ureq::is_test(true);
    /// let resp = ureq::get("http://example.com/")
    ///     .call()?;
    /// # Ok(())
    /// # }
    /// ```

    pub fn call(self) -> Result<Response> {
        let unit = Unit::new(
            &self.agent,
            &self.method,
            &self.url,
            deadline,
        );
        let response = unit::connect(unit).map_err(|e| e.url(self.url.clone()))?;

        let (_version, status, _text) = response.get_status_line()?;

        if status >= 400 {
            Err(Error::Status(status, response))
        } else {
            Ok(response)
        }
    }
    */
}

impl Request {
    pub(crate) fn new(agent: Agent, method: &str, url: &str) -> Result<Request> {
        let method = method.into();
        let url = Url::parse(url.to_owned()).map_err(|_e| ErrorKind::HTTP.new())?;
        Ok(Request {
            agent,
            method,
            url,
        })
    }

    /// Sends the request with no body and blocks the caller until done.
    ///
    /// Use this with GET, HEAD, OPTIONS or TRACE. It sends neither
    /// Content-Length nor Transfer-Encoding.
    ///

    pub fn call(self) -> Result<Response> {
        let timeout = self.agent.config.timeout_connect;
        let deadline = std::time::Instant::now().checked_add(timeout).unwrap();

        let unit = Unit::new(
            &self.agent,
            &self.method,
            &self.url,
            deadline,
        );
        let response = unit::connect(unit).map_err(|e| e.url(self.url.clone()))?;

        let (_version, status, _text) = response.get_status_line()?;

        if status >= 400 {
            Err(Error::Status(status, response))
        } else {
            Ok(response)
        }
    }
}


