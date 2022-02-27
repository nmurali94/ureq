use std::{fmt};

use crate::url::{Url};

use crate::unit::{self, Unit, GetUnits};
use crate::stream::{Stream};
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
	pub agent: Agent,
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
	pub(crate) fn call(&self, urls: Vec<String>) -> Result<Vec<Stream>> {
        let urls: Vec<_> = urls.into_iter()
            .filter_map(|url| Url::parse(url).map_err(|_e| ErrorKind::HTTP.new()).ok())
            .collect();
        let unit = GetUnits::new(
            &self.agent,
            urls.as_slice(),
        );

		println!("Connect");
		let mut streams = unit::connect_v2(unit)?;
		for (url, mut stream) in urls.iter().zip(streams.iter_mut()) {
			unit::send_request(url, &self.agent, &mut stream)?;
		}
		Ok(streams)
	}

	/*
	pub(crate) fn send_request(&self, ) -> Result<()> {
		
	}
	*/
	
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
        );
        let response = unit::connect_v2(unit).map_err(|e| e.url(self.url.clone()))?;

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
        let unit = Unit::new(
            &self.agent,
            &self.method,
            &self.url,
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


