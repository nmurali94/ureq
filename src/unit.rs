use std::fmt::{self, Display};
use std::io::{self, Write};
use std::ops::Range;
use std::time;

use log::debug;
use url::Url;

use crate::error::{Error, ErrorKind};
use crate::header;
use crate::header::{Header};
use crate::response::Response;
use crate::stream::{self, connect_test, Stream};
use crate::Agent;

/// A Unit is fully-built Request, ready to execute.
///
/// *Internal API*
#[derive(Clone)]
pub(crate) struct Unit {
    pub agent: Agent,
    pub method: String,
    pub url: Url,
    headers: Vec<Header>,
    pub deadline: Option<time::Instant>,
}

impl Unit {
    //

    pub(crate) fn new(
        agent: &Agent,
        method: &str,
        url: &Url,
        headers: &[Header],
        deadline: Option<time::Instant>,
    ) -> Self {
        //

        let headers = headers.to_vec();

        Unit {
            agent: agent.clone(),
            method: method.to_string(),
            url: url.clone(),
            headers,
            deadline,
        }
    }

    pub fn is_head(&self) -> bool {
        self.method.eq_ignore_ascii_case("head")
    }

    #[cfg(test)]
    pub fn header(&self, name: &str) -> Option<&str> {
        header::get_header(&self.headers, name)
    }
    #[cfg(test)]
    pub fn has(&self, name: &str) -> bool {
        header::has_header(&self.headers, name)
    }
    #[cfg(test)]
    pub fn all(&self, name: &str) -> Vec<&str> {
        header::get_all_headers(&self.headers, name)
    }
}

/// Perform a connection. Follows redirects.
pub(crate) fn connect(
    mut unit: Unit,
) -> Result<Response, Error> {
    let mut history = vec![];
    let mut resp = loop {
        let resp = connect_inner(&unit, &history)?;

        let (version, status, text) = resp.get_status_line()?;
        // handle redirects
        if !(300..399).contains(&status) || unit.agent.config.redirects == 0 {
            break resp;
        }
        if history.len() + 1 >= unit.agent.config.redirects as usize {
            return Err(ErrorKind::TooManyRedirects.new());
        }
        // the location header
        let location = match resp.header("location") {
            Some(l) => l,
            None => break resp,
        };

        let url = &unit.url;
        let method = &unit.method;
        // join location header to current url in case it is relative
        let new_url = url.join(location).map_err(|e| {
            ErrorKind::InvalidUrl
                .msg(&format!("Bad redirection: {}", location))
                .src(e)
        })?;

        // perform the redirect differently depending on 3xx code.
        let new_method = match status {
            // this is to follow how curl does it. POST, PUT etc change
            // to GET on a redirect.
            301 | 302 | 303 => match &method[..] {
                "GET" | "HEAD" => unit.method,
                _ => "GET".into(),
            },
            // never change the method for 307/308
            // only resend the request if it cannot have a body
            // NOTE: DELETE is intentionally excluded: https://stackoverflow.com/questions/299628
            307 | 308 if ["GET", "HEAD", "OPTIONS", "TRACE"].contains(&method.as_str()) => {
                unit.method
            }
            _ => break resp,
        };
        debug!("redirect {} {} -> {}", status, url, new_url);
        history.push(unit.url.to_string());
        unit.headers.retain(|h| h.name() != "Content-Length");

        // recreate the unit to get a new hostname and cookies for the new host.
        unit = Unit::new(
            &unit.agent,
            &new_method,
            &new_url,
            &unit.headers,
            unit.deadline,
        );
    };
    resp.history = history;
    Ok(resp)
}

/// Perform a connection. Does not follow redirects.
fn connect_inner(
    unit: &Unit,
    previous: &[String],
) -> Result<Response, Error> {
    let host = unit
        .url
        .host_str()
        // This unwrap is ok because Request::parse_url() ensure there is always a host present.
        .unwrap();
    let url = &unit.url;
    let method = &unit.method;
    // open socket
    let mut stream = connect_socket(unit, host)?;

    let send_result = send_prelude(unit, &mut stream, !previous.is_empty());

    if let Err(err) = send_result {
        // not a pooled connection, propagate the error.
        return Err(err.into());
    }

    // start reading the response to process cookies and redirects.
    let result = Response::do_from_request(unit.clone(), stream);

    // https://tools.ietf.org/html/rfc7230#section-6.3.1
    // When an inbound connection is closed prematurely, a client MAY
    // open a new connection and automatically retransmit an aborted
    // sequence of requests if all of those requests have idempotent
    // methods.
    //
    // We choose to retry only requests that used a recycled connection
    // from the ConnectionPool, since those are most likely to have
    // reached a server-side timeout. Note that this means we may do
    // up to N+1 total tries, where N is max_idle_connections_per_host.
    let resp = match result {
        Err(e) => return Err(e),
        Ok(resp) => resp,
    };

    // release the response
    Ok(resp)
}

/// Connect the socket, either by using the pool or grab a new one.
fn connect_socket(unit: &Unit, hostname: &str) -> Result<Stream, Error> {
    match unit.url.scheme() {
        "http" | "https" | "test" => (),
        scheme => return Err(ErrorKind::UnknownScheme.msg(&format!("unknown scheme '{}'", scheme))),
    };
    let stream = match unit.url.scheme() {
        "http" => stream::connect_http(unit, hostname),
        "https" => stream::connect_https(unit, hostname),
        "test" => connect_test(unit),
        scheme => Err(ErrorKind::UnknownScheme.msg(&format!("unknown scheme {}", scheme))),
    }?;
    Ok(stream)
}

/// Send request line + headers (all up until the body).
#[allow(clippy::write_with_newline)]
fn send_prelude(unit: &Unit, stream: &mut Stream, redir: bool) -> io::Result<()> {
    // build into a buffer and send in one go.
    let mut prelude = PreludeBuilder::new();

    // request line
    prelude.write_request_line(
        &unit.method,
        unit.url.path(),
        unit.url.query().unwrap_or_default(),
    )?;

    // host header if not set by user.
    if !header::has_header(&unit.headers, "host") {
        let host = unit.url.host().unwrap();
        match unit.url.port() {
            Some(port) => {
                let scheme_default: u16 = match unit.url.scheme() {
                    "http" => 80,
                    "https" => 443,
                    _ => 0,
                };
                if scheme_default != 0 && scheme_default == port {
                    prelude.write_header("Host", host)?;
                } else {
                    prelude.write_header("Host", format_args!("{}:{}", host, port))?;
                }
            }
            None => {
                prelude.write_header("Host", host)?;
            }
        }
    }
    if !header::has_header(&unit.headers, "user-agent") {
        prelude.write_header("User-Agent", &unit.agent.config.user_agent)?;
    }
    if !header::has_header(&unit.headers, "accept") {
        prelude.write_header("Accept", "*/*")?;
    }

    // other headers
    for header in &unit.headers {
        if !redir || !header.is_name("Authorization") {
            if let Some(v) = header.value() {
                if is_header_sensitive(header) {
                    prelude.write_sensitive_header(header.name(), v)?;
                } else {
                    prelude.write_header(header.name(), v)?;
                }
            }
        }
    }

    // finish
    prelude.finish()?;

    debug!("writing prelude: {}", prelude);
    // write all to the wire
    stream.write_all(prelude.as_slice())?;

    Ok(())
}

fn is_header_sensitive(header: &Header) -> bool {
    header.is_name("Authorization") || header.is_name("Cookie")
}

struct PreludeBuilder {
    prelude: Vec<u8>,
    // Sensitive information to be omitted in debug logging
    sensitive_spans: Vec<Range<usize>>,
}

impl PreludeBuilder {
    fn new() -> Self {
        PreludeBuilder {
            prelude: Vec::with_capacity(256),
            sensitive_spans: Vec::new(),
        }
    }

    fn write_request_line(&mut self, method: &str, path: &str, query: &str) -> io::Result<()> {
        write!(self.prelude, "{} {}", method, path,)?;
        if !query.is_empty() {
            write!(self.prelude, "?{}", query)?;
        }
        write!(self.prelude, " HTTP/1.1\r\n")?;
        Ok(())
    }

    fn write_header(&mut self, name: &str, value: impl Display) -> io::Result<()> {
        write!(self.prelude, "{}: {}\r\n", name, value)
    }

    fn write_sensitive_header(&mut self, name: &str, value: impl Display) -> io::Result<()> {
        write!(self.prelude, "{}: ", name)?;
        let start = self.prelude.len();
        write!(self.prelude, "{}", value)?;
        let end = self.prelude.len();
        self.sensitive_spans.push(start..end);
        write!(self.prelude, "\r\n")?;
        Ok(())
    }

    fn finish(&mut self) -> io::Result<()> {
        write!(self.prelude, "\r\n")
    }

    fn as_slice(&self) -> &[u8] {
        &self.prelude
    }
}

impl fmt::Display for PreludeBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut pos = 0;
        for span in &self.sensitive_spans {
            write!(
                f,
                "{}",
                String::from_utf8_lossy(&self.prelude[pos..span.start])
            )?;
            write!(f, "***")?;
            pos = span.end;
        }
        write!(
            f,
            "{}",
            String::from_utf8_lossy(&self.prelude[pos..]).trim_end()
        )?;
        Ok(())
    }
}
