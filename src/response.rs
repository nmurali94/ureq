use std::fmt;
use std::io::{self, Read};

use chunked_transfer::Decoder as ChunkDecoder;

use crate::error::{Error, ErrorKind, ErrorKind::BadStatus};
use crate::header::Headers;
use crate::readers::*;
use crate::stream::Stream;

use std::convert::{TryFrom};

/// The Response is used to read response headers and decide what to
/// do with the body.  Note that the socket connection is open and the
/// body not read until [`into_reader()`](#method.into_reader)
///

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
            Success => "200 Ok",
            BadRequest => "400 Bad Request",
            NotFound => "404 Not Found",
            InternalServerError => "500 Internal Server Error",
            Unsupported => "Unknown",
        }
    }
}

pub struct Response {
    status: Status,
    headers: Headers,
    reader: ComboReader,
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let status = self.status();
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
    pub fn status(&self) -> Status {
        self.status
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
        let is_close = self
            .header("connection")
            .map(|c| c.eq_ignore_ascii_case("close"))
            .unwrap_or(false);

        let use_chunked = self
            .header("transfer-encoding")
            .map(|enc| !enc.is_empty()) // whatever it says, do chunked
            .unwrap_or(false);

        let limit_bytes = if is_close {
            None
        } else {
            self.header("content-length")
                .and_then(|l| l.parse::<usize>().ok())
        };

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
        //let (mut headers, carryover) = read_status_and_headers(&mut stream)?;
        let b = read_status_and_headers(&mut stream)?;

        let headers = &b.buf[..b.head_len];

        let i = &headers.iter().position(|x| *x == b'\n')
            .ok_or_else(|| ErrorKind::BadStatus.msg("Missing Status Line"))?;
        let status_line = &headers[..i + 1];
        let (_, status) = parse_status_line_from_header(status_line)?;

        let headers = Headers::try_from(&headers[i+1..b.head_len])?;
        //let carryover = b.buf[b.head_len..b.head_len+b.carry_len].try_into().unwrap();

        let reader = ComboReader {
            co: b,
            st: stream,
        };

        Ok(Response {
            status,
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

pub(crate) struct Buffer<const N: usize> {
    pub(crate) buf: [u8; N],
    pub(crate) head_len: usize,
    pub(crate) carry_len: usize,
}

fn read_status_and_headers(reader: &mut Stream) -> io::Result<Buffer<16_384>> {
    let mut buffer = [0; 8192 * 2];
    let mut ri = ReadIterator::<Stream>::new(reader, &mut buffer);

    if let Some(res) = ri.next() {
        let c = res?;
        match &buffer[..c].windows(4).position(|win| win == b"\r\n\r\n") {
            Some(i) => {
                let b = Buffer {
                    buf: buffer,
                    head_len: i+2,
                    carry_len: c-(i+4),
                };
                return Ok(b);
            }
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Failed to fetch HTTP headers in given buffer",
                ));
            }
        }
    }
    Err(io::Error::new(io::ErrorKind::UnexpectedEof,
        "Failed to fetch HTTP headers in given buffer",
    ))
}
