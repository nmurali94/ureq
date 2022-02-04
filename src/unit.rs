use std::io::{self, Write};
use std::time;

//use url::Url;
use crate::url::Url;

use crate::error::{Error, ErrorKind};
use crate::response::Response;
use crate::stream::{self, Stream};
use crate::Agent;

/// A Unit is fully-built Request, ready to execute.
///
/// *Internal API*
pub(crate) struct Unit {
    pub agent: Agent,
    pub method: String,
    pub url: Url,
    pub deadline: time::Instant,
}
pub(crate) struct GetUnits {
    pub agent: Agent,
    pub urls: Vec<Url>,
}

impl Unit {
    //

    pub(crate) fn new(
        agent: &Agent,
        method: &str,
        url: &Url,
        deadline: time::Instant,
    ) -> Self {
        //

        Unit {
            agent: agent.clone(),
            method: method.to_string(),
            url: url.clone(),
            deadline,
        }
    }

    pub fn is_head(&self) -> bool {
        self.method.eq_ignore_ascii_case("head")
    }

}

impl GetUnits {

    pub(crate) fn new(
        agent: &Agent,
        urls: Vec<Url>,
    ) -> Self {
        //

        GetUnits {
            agent: agent.clone(),
            urls: urls,
        }
    }
}

/// Perform a connection. Follows redirects.
pub(crate) fn connect(
    unit: Unit,
) -> Result<Response, Error> {
    //let mut history = HistoryVec::new();
        let resp = connect_inner(unit)?;

        let (_version, status, _text) = resp.get_status_line()?;
        // handle redirects
        if (300..399).contains(&status) {
            println!("Resp {:?}", resp);
            return Err(ErrorKind::TooManyRedirects.new());
        }
    Ok(resp)
}

/// Perform a connection. Does not follow redirects.
fn connect_inner(
    unit: Unit,
) -> Result<Response, Error> {
    // open socket
    let mut stream = connect_socket(&unit)?;

    //let mut buf_stream = BufWriter::with_capacity(256, &mut stream);
    let send_result = send_prelude(&unit, &mut stream);

    if let Err(err) = send_result {
        // not a pooled connection, propagate the error.
        return Err(err.into());
    }

    // start reading the response to process cookies and redirects.
    //drop(buf_stream);
    let result = Response::do_from_request(unit, stream);

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
fn connect_sockets(units: &[GetUnits])  {
}
/// Connect the socket, either by using the pool or grab a new one.
fn connect_socket(unit: &Unit) -> Result<Stream, Error> {
    match unit.url.scheme() {
        "http" | "https" => (),
        scheme => return Err(ErrorKind::UnknownScheme.msg(&format!("unknown scheme '{}'", scheme))),
    };
    let stream = match unit.url.scheme() {
        "http" => stream::connect_http(unit),
        "https" => stream::connect_https(unit),
        scheme => Err(ErrorKind::UnknownScheme.msg(&format!("unknown scheme {}", scheme))),
    }?;
    Ok(stream)
}

/// Send request line + headers (all up until the body).
#[allow(clippy::write_with_newline)]
fn send_prelude(unit: &Unit, stream: &mut Stream) -> io::Result<()> {

    // request line
    let mut v = arrayvec::ArrayVec::<u8, 512>::new_const();
    
    let _ = v.try_extend_from_slice(b"GET ");
    let _ = v.try_extend_from_slice(unit.url.path().as_bytes());
    let _ = v.try_extend_from_slice(b" HTTP/1.1\r\n");

    // host header if not set by user.
    let _ = v.try_extend_from_slice(b"Host: ");
    let _ = v.try_extend_from_slice(unit.url.host_str().as_bytes());
    let _ = v.try_extend_from_slice(b"\r\n");

    let _ = v.try_extend_from_slice(b"User-Agent: ");
    let _ = v.try_extend_from_slice(unit.agent.config.user_agent.as_bytes());
    let _ = v.try_extend_from_slice(b"\r\n");

    // finish

    let _ = v.try_extend_from_slice(b"\r\n");
    /*
    let mut arr = [0u8; 2048];
    let c = (&mut arr[..]).write_vectored(&v)?;
    println!("Arr \n{}", std::str::from_utf8(&arr[..c]).unwrap());
    */

    stream.write_all(&v)?;
    // write all to the wire

    Ok(())
}
