use std::fmt;
use std::io::{self, Read};

use chunked_transfer::Decoder as ChunkDecoder;

use crate::error::{Error, ErrorKind, ErrorKind::BadStatus};
use crate::header::Headers;
use crate::readers::*;
use crate::stream::Stream;

use std::convert::{TryFrom, TryInto};

/// Response instances are created as results of firing off requests.
/// The `Response` is used to read response headers and decide what to do with the body.  Note that the socket connection is open and the body not read until [`into_reader()`](#method.into_reader)
///

type StatusVec = arrayvec::ArrayVec<u8, 32>;
type BufVec = arrayvec::ArrayVec<u8, 2048>;
type CarryOver = arrayvec::ArrayVec<u8, 2048>;

#[derive(Clone, Copy)]
pub enum Status {
    Success = 200,
    BadRequest = 400,
    NotFound = 404,
    InternalServerError = 500,
    Unsupported,
}

impl From<u16> for Status {
    fn from(n: u16) -> Self {
        use Status::*;
        match n {
            200 => Success,
            400 => BadRequest,
            404 => NotFound,
            500 => InternalServerError,
            _ => Unsupported,
        }
    }
}

impl Status {
    pub fn to_str(self) -> &'static str {
        use Status::*;
        match self {
            Success => "Ok",
            BadRequest => "Bad Request",
            NotFound => "Not Found",
            InternalServerError => "Internal Server Error",
            Unsupported => "Unknown",
        }
    }
}

pub struct Response {
    status_line: StatusVec,
    headers: Headers,
    reader: ComboReader,
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (_version, status) = self.get_status_line().unwrap();
        let text = status.to_str();
        let status = status as u16;
        write!(f, "Response[status: {}, status_text: {}", status, text,)?;
        write!(f, "]")
    }
}

enum RR {
    C(ChunkDecoder<ComboReader>),
    L(std::io::Take<ComboReader>),
    R(ComboReader),
}

// Cannot RR directly because it would leak ComboReader to the consumer
pub struct ResponseReader(RR);

impl Read for ResponseReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        use RR::*;
        match &mut self.0 {
            C(c) => c.read(buf),
            L(c) => c.read(buf),
            R(c) => c.read(buf),
        }
    }
}

impl ResponseReader {
    pub fn read_to_end(mut self, data: &mut [u8]) -> io::Result<&mut [u8]> {
        ReadToEndIterator::<Self>::new(&mut self, data)
            .try_fold(0, |acc, r| r.map(|c| acc + c))
            .map(move |st| &mut data[..st])
    }
}

impl Response {
    pub fn get_status_line(&self) -> Result<(&'static str, Status), Error> {
        parse_status_line_from_header(&self.status_line)
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .header(name)
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
    pub fn into_reader(self) -> ResponseReader {
        //
        let (http_version, _) = self.get_status_line().unwrap();
        let is_http10 = http_version.eq_ignore_ascii_case("HTTP/1.0");
        let is_close = self
            .header("connection")
            .map(|c| c.eq_ignore_ascii_case("close"))
            .unwrap_or(false);

        let is_chunked = self
            .header("transfer-encoding")
            .map(|enc| !enc.is_empty()) // whatever it says, do chunked
            .unwrap_or(false);

        let use_chunked = !is_http10 && is_chunked;

        let limit_bytes = if is_http10 || is_close {
            None
        } else {
            self.header("content-length")
                .and_then(|l| l.parse::<usize>().ok())
        };
        //println!("Limit = {} {:?}, {}", use_chunked, limit_bytes, self.carryover.len());

        use RR::*;
        let rr = match (use_chunked, limit_bytes) {
            (true, _) => C(ChunkDecoder::new(self.reader)),
            (false, Some(len)) => L(self.reader.take(len as u64)),
            (false, None) => R(self.reader),
        };

        ResponseReader(rr)
    }

    pub(crate) fn do_from_stream(mut stream: Stream) -> Result<Response, Error> {
        //
        // HTTP/1.1 200 OK\r\n
        let (mut headers, carryover) = read_status_and_headers(&mut stream)?;

        let i = memchr::memchr(b'\n', &headers)
            .ok_or_else(|| ErrorKind::BadStatus.msg("Missing Status Line"))?;
        let status_line: StatusVec = headers.drain(..i + 1).collect();
        //println!("Status: {}", std::str::from_utf8(&status_line).unwrap());

        //println!("Headers: {}", std::str::from_utf8(&headers).unwrap());
        let headers = Headers::try_from(headers)?;

        let reader = ComboReader {
            co: carryover,
            st: stream,
        };

        Ok(Response {
            status_line,
            headers,
            reader,
        })
    }
}

// HTTP/1.1 200 OK\r\n
fn parse_status_line_from_header(s: &[u8]) -> Result<(&'static str, Status), Error> {
    if s.len() < 12 {
        Err(BadStatus.msg("Status line isn't formatted correctly"))
    } else if b"HTTP/1.1 " != &s[..9] {
        Err(BadStatus.msg("HTTP version not formatted correctly"))
    } else if s[9..12].iter().any(|c| !c.is_ascii_digit()) || s[12] != b' ' {
        Err(BadStatus.msg("HTTP status code must be a 3 digit number"))
    } else {
        let status =
            ((s[9] - b'0') as u16 * 100) + (s[10] - b'0') as u16 * 10 + (s[11] - b'0') as u16;
        let status = Status::from(status);
        std::str::from_utf8(&s[12..])
            .map_err(|_| BadStatus.new())
            .map(|_| ("HTTP/1.1", status))
    }
}

fn read_status_and_headers(reader: &mut Stream) -> io::Result<(BufVec, CarryOver)> {
    let mut buffer = [0; 2048];
    let mut ri = ReadIterator::<Stream>::new(reader, &mut buffer);

    if let Some(res) = ri.next() {
        let c = res?;
        match memchr::memmem::find(&buffer[..c], b"\r\n\r\n") {
            Some(i) => {
                let buf: BufVec = buffer[..i + 2].try_into().unwrap();
                buffer.copy_within(i + 4..c, 0);
                let carry = c - i - 4;
                let carryover: CarryOver = buffer[..carry].try_into().unwrap();
                return Ok((buf, carryover));
            }
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Failed to fetch HTTP headers in given buffer",
                ));
            }
        }
    }
    Ok((BufVec::new(), CarryOver::new()))
}
