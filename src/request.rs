use std::{fmt};

use url::{Url};

use crate::header::{Header};
use crate::unit::{self, Unit};
use crate::Response;
use crate::{agent::Agent, error::Error};

#[cfg(feature = "json")]
use super::SerdeValue;

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
    headers: Vec<Header>,
}

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Request({} {}, {:?})",
            self.method, self.url, self.headers
        )
    }
}

impl Request {
    pub(crate) fn new(agent: Agent, method: &str, url: &str) -> Result<Request> {
        let method = method.into();
        let url = Url::parse(url)?;
        Ok(Request {
            agent,
            method,
            url,
            headers: vec![],
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_implements_send_and_sync() {
        let _request: Box<dyn Send> = Box::new(Request::new(
            Agent::new(),
            "GET".to_string(),
            "https://example.com/".to_string(),
        ));
        let _request: Box<dyn Sync> = Box::new(Request::new(
            Agent::new(),
            "GET".to_string(),
            "https://example.com/".to_string(),
        ));
    }

    #[test]
    fn send_byte_slice() {
        let bytes = vec![1, 2, 3];
        crate::agent()
            .post("http://example.com")
            .send(&bytes[1..2])
            .ok();
    }

    #[test]
    fn disallow_empty_host() {
        let req = crate::agent().get("file:///some/path");

        // Both request_url and call() must surface the same error.
        assert_eq!(
            req.request_url().unwrap_err().kind(),
            crate::ErrorKind::InvalidUrl
        );

        assert_eq!(req.call().unwrap_err().kind(), crate::ErrorKind::InvalidUrl);
    }
}
