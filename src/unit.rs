use std::io::{self, Write};

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
}
pub(crate) struct GetUnits<'a> {
    pub agent: Agent,
    pub urls: &'a [Url],
}

impl Unit {
    //

    pub(crate) fn new(
        agent: &Agent,
        method: &str,
        url: &Url,
    ) -> Self {
        //

        Unit {
            agent: agent.clone(),
            method: method.to_string(),
            url: url.clone(),
        }
    }
}

impl<'a> GetUnits<'a> {

    pub(crate) fn new(
        agent: &Agent,
        urls: &'a [Url],
    ) -> Self {
        //

        GetUnits {
            agent: agent.clone(),
            urls,
        }
    }
}
/// Perform a connection. Follows redirects.
pub(crate) fn connect_v2(
    units: GetUnits,
) -> Result<Vec<Stream>, Error> {
    //let mut history = HistoryVec::new();
	connect_sockets(&units)
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

    let send_result = send_request(&unit.url, &unit.agent, &mut stream);

    if let Err(err) = send_result {
        // not a pooled connection, propagate the error.
        return Err(err.into());
    }

    // start reading the response to process cookies and redirects.
    let result = Response::do_from_stream(stream);

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
fn connect_sockets(units: &GetUnits) -> Result<Vec<Stream>, Error> {
	let p: Vec<_> = units.urls.iter().map(|u| if u.scheme() == "https" { 443 } else { 80 }).collect();
	let streams = stream::connect_http_v2(&units.urls, p.as_slice())?;
	let mut ss = Vec::new();
	for (i, (stream, url)) in streams.into_iter().zip(units.urls.iter()).enumerate() {
		let s = if url.scheme() == "https" {
			stream::connect_https_v2(stream, units.urls[i].host_str(), &units.agent)?
		} else {
			Stream::from_tcp_stream(stream)
		};
		ss.push(s);
	}
	Ok(ss)
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
pub(crate) fn send_request(url: &Url, agent: &Agent, stream: &mut Stream) -> io::Result<()> {

    // request line
    let mut v = arrayvec::ArrayVec::<u8, 512>::new_const();
    
    let _ = v.try_extend_from_slice(b"GET ");
    let _ = v.try_extend_from_slice(url.path().as_bytes());
    let _ = v.try_extend_from_slice(b" HTTP/1.1\r\n");

    // host header if not set by user.
    let _ = v.try_extend_from_slice(b"Host: ");
    let _ = v.try_extend_from_slice(url.host_str().as_bytes());
    let _ = v.try_extend_from_slice(b"\r\n");

    let _ = v.try_extend_from_slice(b"User-Agent: ");
    let _ = v.try_extend_from_slice(agent.config.user_agent.as_bytes());
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
