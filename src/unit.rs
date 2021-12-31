use std::io::{self, Write};
use std::time;
use std::convert::TryInto;
use std::io::{BufWriter, IoSlice};

use log::debug;
//use url::Url;
use crate::url::Url;

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
    headers: HeaderVec,
    pub deadline: time::Instant,
}
//type HistoryVec = arrayvec::ArrayVec<Url, 8>;
type HeaderVec = arrayvec::ArrayVec<Header, 16>;

impl Unit {
    //

    pub(crate) fn new(
        agent: &Agent,
        method: &str,
        url: &Url,
        headers: &[Header],
        deadline: time::Instant,
    ) -> Self {
        //

        let headers = headers.try_into().unwrap();

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
    //let mut history = HistoryVec::new();
        let resp = connect_inner(&unit, true)?;

        let (_version, status, _text) = resp.get_status_line()?;
        // handle redirects
        if (300..399).contains(&status) {
            println!("Resp {:?}", resp);
            std::process::exit(-1);
            return Err(ErrorKind::TooManyRedirects.new());
        }
    Ok(resp)
}

/// Perform a connection. Does not follow redirects.
fn connect_inner(
    unit: &Unit,
    empty_previous: bool,
) -> Result<Response, Error> {
    let host = unit
        .url
        .host_str()
        ;
    // open socket
    let mut stream = connect_socket(unit, host)?;
    let mut buf_stream = BufWriter::with_capacity(256, &mut stream);
    let send_result = send_prelude(unit, &mut buf_stream, !empty_previous);


    if let Err(err) = send_result {
        // not a pooled connection, propagate the error.
        return Err(err.into());
    }

    // start reading the response to process cookies and redirects.
    drop(buf_stream);
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
fn send_prelude(unit: &Unit, stream: &mut BufWriter<&mut Stream>, redir: bool) -> io::Result<()> {

    // request line
    let mut v = arrayvec::ArrayVec::<IoSlice, 64>::new();
    
    v.push(IoSlice::new(unit.method.as_bytes()));
    v.push(IoSlice::new(b" "));
    v.push(IoSlice::new(unit.url.path().as_bytes()));
    if unit.url.query().is_some() {
        v.push(IoSlice::new(b"?"));
        v.push(IoSlice::new(unit.url.query().unwrap().as_bytes()));
    }
    v.push(IoSlice::new(b" HTTP/1.1\r\n"));

    // host header if not set by user.
    if !header::has_header(&unit.headers, "host") {
        v.push(IoSlice::new(b"Host: "));
        v.push(IoSlice::new(&unit.url.host_str().as_bytes()));
        v.push(IoSlice::new(b"\r\n"));
    }
    if !header::has_header(&unit.headers, "user-agent") {
        v.push(IoSlice::new(b"User-Ager: "));
        v.push(IoSlice::new(&unit.agent.config.user_agent.as_bytes()));
        v.push(IoSlice::new(b"\r\n"));
    }
    if !header::has_header(&unit.headers, "accept") {
        v.push(IoSlice::new(b"Accept: */*\r\n"));
    }

    // other headers
    for header in &unit.headers {
        if !redir || !header.is_name("Authorization") {
            if let Some(val) = header.value() {
                    v.push(IoSlice::new(header.name().as_bytes()));
                    v.push(IoSlice::new(b": "));
                    v.push(IoSlice::new(val.as_bytes()));
                    v.push(IoSlice::new(b"\r\n"));
            }
        }
    }

    // finish

    v.push(IoSlice::new(b"\r\n"));
    /*
    let mut arr = [0u8; 2048];
    let c = (&mut arr[..]).write_vectored(&v)?;
    println!("Arr \n{}", std::str::from_utf8(&arr[..c]).unwrap());
    */

    let _ = stream.write_vectored(&v)?;
    // write all to the wire
    let _ = stream.flush()?;

    Ok(())
}
