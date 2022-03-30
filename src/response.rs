use std::io::{self, Read};
use std::{fmt};

use chunked_transfer::Decoder as ChunkDecoder;

use crate::error::{Error, ErrorKind::BadStatus};
use crate::header::{Headers};
use crate::stream::{Stream};
use crate::{ErrorKind};

use std::convert::TryFrom;

/// Response instances are created as results of firing off requests.
///
/// The `Response` is used to read response headers and decide what to do with the body.
/// Note that the socket connection is open and the body not read until one of
/// [`into_reader()`](#method.into_reader), [`into_json()`](#method.into_json), or
///
/// When dropping a `Response` instance, one one of two things can happen. If
/// the response has unread bytes, the underlying socket cannot be reused,
/// and the connection is closed. If there are no unread bytes, the connection
/// is returned to the [`Agent`](crate::Agent) connection pool used (notice there is always
/// an agent present, even when not explicitly configured by the user).
///

type StatusVec = arrayvec::ArrayVec<u8, 32>;
type BufVec = arrayvec::ArrayVec<u8, 2048>;
type CarryOver = arrayvec::ArrayVec<u8, 2048>;

pub struct Response {
    status_line: StatusVec,
    headers: Headers,
    reader: ComboReader,
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (_version, status, text) = self.get_status_line().unwrap();
        write!(
            f,
            "Response[status: {}, status_text: {}",
            status,
            text,
            )?;
        write!(f, "]")
    }
}

impl Response {

    pub fn get_status_line(&self) -> Result<(&str, u16, &str), Error> {
        parse_status_line_from_header(&self.status_line)
    }

    /// The header value for the given name, or None if not found.
    ///
    /// For historical reasons, the HTTP spec allows for header values
    /// to be encoded using encodigs like iso-8859-1. Such encodings
    /// means the values are not possible to interpret as utf-8.
    ///
    /// In case the header value can't be read as utf-8, this function
    /// returns `None` (while the name is visible in [`Response::headers_names()`]).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.header(name)
            .and_then(|s| std::str::from_utf8(s).ok())
            .map(|s| s.trim())
    }

    /// Turn this response into a `impl Read` of the body.
    ///
    /// 1. If `Transfer-Encoding: chunked`, the returned reader will unchunk it
    ///    and any `Content-Length` header is ignored.
    /// 2. If `Content-Length` is set, the returned reader is limited to this byte
    ///    length regardless of how many bytes the server sends.
    /// 3. If no length header, the reader is until server stream end.
    ///
    /// Note: If you use `read_to_end()` on the resulting reader, a malicious
    /// server might return enough bytes to exhaust available memory. If you're
    /// making requests to untrusted servers, you should use `.take()` to
    /// limit the response bytes read.
    ///
    /// Example:
    ///
    pub fn into_reader(self) -> Box<dyn Read + Send + Sync> {
        //
        let (http_version, status, _status_text) = self.get_status_line().unwrap();
        let is_http10 = http_version.eq_ignore_ascii_case("HTTP/1.0");
        let is_close = self
            .header("connection")
            .map(|c| c.eq_ignore_ascii_case("close"))
            .unwrap_or(false);

        let has_no_body = match status {
                204 | 304 => true,
                _ => false,
            };

        let is_chunked = self
            .header("transfer-encoding")
            .map(|enc| !enc.is_empty()) // whatever it says, do chunked
            .unwrap_or(false);

        let use_chunked = !is_http10 && !has_no_body && is_chunked;

        let limit_bytes = if is_http10 || is_close {
            None
        } else if has_no_body {
            // head requests never have a body
            Some(0)
        } else {
            self.header("content-length")
                .and_then(|l| l.parse::<usize>().ok())
        };
        //println!("Limit = {} {:?}, {}", use_chunked, limit_bytes, self.carryover.len());


        match (use_chunked, limit_bytes) {
            (true, _) => Box::new(ChunkDecoder::new(self.reader)),
            (false, Some(len)) => Box::new(self.reader.take(len as u64)),
            (false, None) => Box::new(self.reader),
        }
    }

    pub(crate) fn do_from_stream(mut stream: Stream) -> Result<Response, Error> {
        //
        // HTTP/1.1 200 OK\r\n
        let (mut headers, carryover) = read_status_and_headers(&mut stream)?;

        let i = memchr::memchr(b'\n', &headers)
		    .ok_or_else(||ErrorKind::BadStatus.msg("Missing Status Line"))?;
        let status_line: StatusVec = headers.drain(..i+1).collect();
        //println!("Status: {}", std::str::from_utf8(&status_line).unwrap());

        //println!("Headers: {}", std::str::from_utf8(&headers).unwrap());
        let headers = Headers::try_from(headers)?;

        let reader = ComboReader::new(carryover, stream);

        Ok(Response {
            status_line,
            headers,
            reader,
        })
    }
}

struct ComboReader {
    co: CarryOver,
    st: Stream,
}

impl ComboReader {
    fn new(a: CarryOver, b: Stream) -> Self {
        ComboReader {
            co: a,
            st: b,
        }
    }
} 

impl Read for ComboReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let c = self.co.as_slice().read(buf)?;
        if c == 0 {
            self.st.read(buf)
        }
        else { 
            let _ = self.co.drain(..c);
            Ok(c)
        }
    }
}

// HTTP/1.1 200 OK\r\n
fn parse_status_line_from_header(s: &[u8]) -> Result<(&str, u16, &str), Error> {
    if s.len() < 12 {
        Err(BadStatus.msg("Status line isn't formatted correctly"))
    }
    else if b"HTTP/1.1 " != &s[..9] {
        Err(BadStatus.msg("HTTP version not formatted correctly"))
    }
    else if s[9..12].iter().any(|c| !c.is_ascii_digit()) || s[12] != b' ' {
        Err(BadStatus.msg("HTTP status code must be a 3 digit number"))
    }
    else {
		let status = ((s[9] - b'0') as u16 * 100)  + (s[10] - b'0') as u16 * 10 + (s[11] - b'0') as u16;
        std::str::from_utf8(&s[12..]).map_err(|_| BadStatus.new())
			.map(|text| {
	        (
	            "HTTP/1.1",
	            status,
	            text,
	            
	        )
		})
    }
}

fn read_status_and_headers(reader: &mut impl Read) -> io::Result<(BufVec, CarryOver)> {
    let mut buf = BufVec::new_const();
    let mut buffer = [0u8; 2048];

    let mut carry = 0;

    loop {
        let r = reader.read(&mut buffer[carry..]);

        let mut c = match r {
            Ok(n) => n,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if c == 0 {
            break;
        }
        c += carry;
        let crlf = memchr::memmem::find(&buffer[..c], b"\r\n\r\n");
        match crlf {
            Some(i) => {
                let _ = buf.try_extend_from_slice(&buffer[..i+2]);
                buffer.copy_within(i+4..c, 0);
                carry = c - i - 4;
                break;
            }
            None => {
                let _ = buf.try_extend_from_slice(&buffer[..c - 3]);
                buffer.copy_within(c - 3..c, 0);
                carry = 3;
            }
        }
    }

    let mut carryover = CarryOver::new_const();
    let _ = carryover.try_extend_from_slice(&buffer[..carry]).unwrap();
    Ok((buf, carryover))
}

// ErrorReader returns an error for every read.
// The error is as close to a clone of the underlying
// io::Error as we can get.
struct ErrorReader(io::Error);

impl Read for ErrorReader {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(self.0.kind(), self.0.to_string()))
    }
}

