use std::{fmt, time};

use url::{ParseError, Url};

use crate::body::Payload;
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
    pub(crate) fn new(agent: Agent, method: String, url: String) -> Request {
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
    pub fn call(self) -> Result<Response> {
        self.do_call(Payload::Empty)
    }

    fn parse_url(&self) -> Result<Url> {
        Ok(self.url.parse().and_then(|url: Url|
            // No hostname is fine for urls in general, but not for website urls.
            if url.host_str().is_none() {
                Err(ParseError::EmptyHost)
            } else {
                Ok(url)
            }
        )?)
    }

    fn do_call(self, payload: Payload) -> Result<Response> {
        for h in &self.headers {
            h.validate()?;
        }
        let url = self.parse_url()?;

        let deadline = match self.timeout.or(self.agent.config.timeout) {
            None => None,
            Some(timeout) => {
                let now = time::Instant::now();
                Some(now.checked_add(timeout).unwrap())
            }
        };

        let reader = payload.into_read();
        let unit = Unit::new(
            &self.agent,
            &self.method,
            &url,
            &self.headers,
            &reader,
            deadline,
        );
        let response = unit::connect(unit, true, reader).map_err(|e| e.url(url.clone()))?;

        if response.status() >= 400 {
            Err(Error::Status(response.status(), response))
        } else {
            Ok(response)
        }
    }
}

/// Parsed result of a request url with handy inspection methods.
#[derive(Debug, Clone)]
pub struct RequestUrl {
    url: Url,
    query_pairs: Vec<(String, String)>,
}

impl RequestUrl {

    /// Handle the request url as a standard [`url::Url`].
    pub fn as_url(&self) -> &Url {
        &self.url
    }

    /// Get the scheme of the request url, i.e. "https" or "http".
    pub fn scheme(&self) -> &str {
        self.url.scheme()
    }

    /// Host of the request url.
    pub fn host(&self) -> &str {
        // this unwrap() is ok, because RequestUrl is tested for empty host
        // urls in Request::parse_url().
        self.url.host_str().unwrap()
    }

    /// Port of the request url, if available. Ports are only available if they
    /// are present in the original url. Specifically the scheme default ports,
    /// 443 for `https` and and 80 for `http` are `None` unless explicitly
    /// set in the url, i.e. `https://my-host.com:443/some/path`.
    pub fn port(&self) -> Option<u16> {
        self.url.port()
    }

    /// Path of the request url.
    pub fn path(&self) -> &str {
        self.url.path()
    }

    /// Returns all query parameters as a vector of key-value pairs.
    ///
    /// ```
    /// # fn main() -> Result<(), ureq::Error> {
    /// # ureq::is_test(true);
    /// let req = ureq::get("http://httpbin.org/get")
    ///     .query("foo", "42")
    ///     .query("foo", "43");
    ///
    /// assert_eq!(req.request_url().unwrap().query_pairs(), vec![
    ///     ("foo", "42"),
    ///     ("foo", "43")
    /// ]);
    /// # Ok(())
    /// # }
    /// ```
    pub fn query_pairs(&self) -> Vec<(&str, &str)> {
        self.query_pairs
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
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
