use std::{fmt};

use crate::url::{Url};

use crate::header::{Header};
use crate::unit::{self, Unit};
use crate::Response;
use crate::{agent::Agent, error::Error, error::ErrorKind};

pub type Result<T> = std::result::Result<T, Error>;

/// Request instances are builders that creates a request.
///
/// ```
/// # fn main() -> Result<(), ureq::Error> {
/// # ureq::is_test(true);
/// let response = ureq::get("http://example.com/form")
///     .query("foo", "bar baz")  // add ?foo=bar+baz
///     .call()?;                 // run the request
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Request {
    agent: Agent,
    method: String,
    url: Url,
    headers: HeaderVec,
}
type HeaderVec = arrayvec::ArrayVec<Header, 16>;

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Request({} {:?}, {:?})",
            self.method, self.url, self.headers
        )
    }
}

impl Request {
    pub(crate) fn new(agent: Agent, method: &str, url: &str) -> Result<Request> {
        let method = method.into();
        let url = Url::parse(url.to_owned()).map_err(|_e| ErrorKind::HTTP.new())?;
        Ok(Request {
            agent,
            method,
            url,
            headers: HeaderVec::new(),
        })
    }

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
        for h in &self.headers {
            h.validate()?;
        }

        let timeout = self.agent.config.timeout_connect;
        let deadline = std::time::Instant::now().checked_add(timeout).unwrap();

        let unit = Unit::new(
            &self.agent,
            &self.method,
            &self.url,
            &self.headers,
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


