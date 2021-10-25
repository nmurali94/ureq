use std::{fmt, time};

use url::{ParseError, Url};

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
    url: String,
    headers: Vec<Header>,
    timeout: Option<time::Duration>,
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
    pub(crate) fn new(agent: Agent, method: &str, url: &str) -> Request {
        let method = method.into();
        let url = url.into();
        Request {
            agent,
            method,
            url,
            headers: vec![],
            timeout: None,
        }
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
    fn parse_url(&self) -> Result<Url> {
        Ok(self.url.parse().and_then(|url: Url|
            // No hostname is fine for urls in general, but not for website urls.
            if url.host_str().is_none() {
                Err(ParseError::EmptyHost)
            } else {
                Ok(url)
            })?)
    }

    pub fn call(self) -> Result<Response> {
        for h in &self.headers {
            h.validate()?;
        }
        let url = self.parse_url()?;

        let deadline = None;

        let unit = Unit::new(
            &self.agent,
            &self.method,
            &url,
            &self.headers,
            deadline,
        );
        let response = unit::connect(unit).map_err(|e| e.url(url.clone()))?;

        if response.status() >= 400 {
            Err(Error::Status(response.status(), response))
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
