use std::io::{self, Read};
use std::{fmt};

use chunked_transfer::Decoder as ChunkDecoder;
//use url::Url;

use crate::error::{Error, ErrorKind::BadStatus};
use crate::header::{Headers};
use crate::stream::{Stream, time_until_deadline};
use crate::unit::Unit;
use crate::{ErrorKind};

use std::convert::TryFrom;

#[cfg(feature = "json")]
use serde::de::DeserializeOwned;

#[cfg(feature = "charset")]
use encoding_rs::Encoding;

pub const DEFAULT_CONTENT_TYPE: &str = "text/plain";
pub const DEFAULT_CHARACTER_SET: &str = "utf-8";

/// Response instances are created as results of firing off requests.
///
/// The `Response` is used to read response headers and decide what to do with the body.
/// Note that the socket connection is open and the body not read until one of
/// [`into_reader()`](#method.into_reader), [`into_json()`](#method.into_json), or
/// [`into_string()`](#method.into_string) consumes the response.
///
/// When dropping a `Response` instance, one one of two things can happen. If
/// the response has unread bytes, the underlying socket cannot be reused,
/// and the connection is closed. If there are no unread bytes, the connection
/// is returned to the [`Agent`](crate::Agent) connection pool used (notice there is always
/// an agent present, even when not explicitly configured by the user).
///
/// ```
/// # fn main() -> Result<(), ureq::Error> {
/// # ureq::is_test(true);
/// let response = ureq::get("http://example.com/").call()?;
///
/// // socket is still open and the response body has not been read.
///
/// let text = response.into_string()?;
///
/// // response is consumed, and body has been read.
/// # Ok(())
/// # }
/// ```

type StatusVec = arrayvec::ArrayVec<u8, 32>;
//type HistoryVec = arrayvec::ArrayVec<Url, 8>;
type BufVec = arrayvec::ArrayVec<u8, 4096>;
type CarryOver = arrayvec::ArrayVec<u8, 4096>;

pub struct Response {
    status_line: StatusVec,
    headers: Headers,
    // Boxed to avoid taking up too much size.
    unit: Unit,
    // Boxed to avoid taking up too much size.
    stream: Stream,
    carryover: CarryOver,
    //pub(crate) history: HistoryVec,
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
        write!(f, ", url: {:?}", self.unit.url)?;
        write!(f, "]")
    }
}

impl Response {
    /// Construct a response with a status, status text and a string body.
    ///
    /// This is hopefully useful for unit tests.
    ///
    /// Example:
    ///
    /// ```
    /// # fn main() -> Result<(), ureq::Error> {
    /// # ureq::is_test(true);
    /// let resp = ureq::Response::new(401, "Authorization Required", "Please log in")?;
    ///
    /// assert_eq!(resp.status(), 401);
    /// # Ok(())
    /// # }
    /// ```
    /*
    pub fn new(status: u16, status_text: &str, body: &str) -> Result<Response, Error> {
        let r = format!("HTTP/1.1 {} {}\r\n\r\n{}", status, status_text, body);
        (r.as_ref() as &str).parse()
    }
    */

    /// The URL we ended up at. This can differ from the request url when
    /// we have followed redirects.
    pub fn get_url(&self) -> &str {
        self.unit.url.as_str()
    }

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
            .and_then(|s| Some(s.trim()))
    }

    /// The content type part of the "Content-Type" header without
    /// the charset.
    ///
    /// Example:
    ///
    /// ```
    /// # fn main() -> Result<(), ureq::Error> {
    /// # ureq::is_test(true);
    /// let resp = ureq::get("http://example.com/").call()?;
    /// assert!(matches!(resp.header("content-type"), Some("text/html; charset=ISO-8859-1")));
    /// assert_eq!("text/html", resp.content_type());
    /// # Ok(())
    /// # }
    /// ```
    pub fn content_type(&self) -> &str {
        self.header("content-type")
            .map(|header| {
                header
                    .find(';')
                    .map(|index| &header[0..index])
                    .unwrap_or(header)
            })
        .unwrap_or(DEFAULT_CONTENT_TYPE)
    }

    /// The character set part of the "Content-Type".
    ///
    /// Example:
    ///
    /// ```
    /// # fn main() -> Result<(), ureq::Error> {
    /// # ureq::is_test(true);
    /// let resp = ureq::get("http://example.com/").call()?;
    /// assert!(matches!(resp.header("content-type"), Some("text/html; charset=ISO-8859-1")));
    /// assert_eq!("ISO-8859-1", resp.charset());
    /// # Ok(())
    /// # }
    /// ```
    pub fn charset(&self) -> &str {
        charset_from_content_type(self.header("content-type"))
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
    /// ```
    /// use std::io::Read;
    /// # fn main() -> Result<(), ureq::Error> {
    /// # ureq::is_test(true);
    /// let resp = ureq::get("http://httpbin.org/bytes/100")
    ///     .call()?;
    ///
    /// assert!(resp.has("Content-Length"));
    /// let len = resp.header("Content-Length")
    ///     .and_then(|s| s.parse::<usize>().ok()).unwrap();
    ///
    /// let mut bytes: Vec<u8> = Vec::with_capacity(len);
    /// resp.into_reader()
    ///     .take(10_000_000)
    ///     .read_to_end(&mut bytes)?;
    ///
    /// assert_eq!(bytes.len(), len);
    /// # Ok(())
    /// # }
    /// ```
    pub fn into_reader(self) -> (impl Read + Send, CarryOver) {
        //
        let (http_version, status, _status_text) = self.get_status_line().unwrap();
        let is_http10 = http_version.eq_ignore_ascii_case("HTTP/1.0");
        let is_close = self
            .header("connection")
            .map(|c| c.eq_ignore_ascii_case("close"))
            .unwrap_or(false);

        let is_head = self.unit.is_head();
        let has_no_body = is_head
            || match status {
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

        let stream = self.stream;
        let unit = self.unit;
            let result = time_until_deadline(unit.deadline);
            if let Err(e) = result {
                return (Box::new(ErrorReader(e)) as Box<dyn Read + Send>, self.carryover);
            }

        match (use_chunked, limit_bytes) {
            (true, _) => (Box::new(ChunkDecoder::new(stream)), self.carryover),
            (false, Some(len)) => {
                (Box::new(LimitedRead::new(stream, len - self.carryover.len())), self.carryover)
            }
            (false, None) => (Box::new(stream), self.carryover),
        }
    }

    /// Turn this response into a String of the response body. By default uses `utf-8`,
    /// but can work with charset, see below.
    ///
    /// This is potentially memory inefficient for large bodies since the
    /// implementation first reads the reader to end into a `Vec<u8>` and then
    /// attempts to decode it using the charset.
    ///
    /// If the response is larger than 10 megabytes, this will return an error.
    ///
    /// Example:
    ///
    /// ```
    /// # fn main() -> Result<(), ureq::Error> {
    /// # ureq::is_test(true);
    /// let text = ureq::get("http://httpbin.org/get/success")
    ///     .call()?
    ///     .into_string()?;
    ///
    /// assert!(text.contains("success"));
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## Charset support
    ///
    /// If you enable feature `ureq = { version = "*", features = ["charset"] }`, into_string()
    /// attempts to respect the character encoding of the `Content-Type` header. If there is no
    /// Content-Type header, or the Content-Type header does not specify a charset, into_string()
    /// uses `utf-8`.
    ///
    /// I.e. `Content-Length: text/plain; charset=iso-8859-1` would be decoded in latin-1.
    ///
    /*
    pub fn into_string(self) -> io::Result<String> {
        #[cfg(feature = "charset")]
        let encoding = Encoding::for_label(self.charset().as_bytes())
            .or_else(|| Encoding::for_label(DEFAULT_CHARACTER_SET.as_bytes()))
            .unwrap();

        let mut buf: Vec<u8> = vec![];
        self.into_reader()
            .take((INTO_STRING_LIMIT + 1) as u64)
            .read_to_end(&mut buf)?;
        if buf.len() > INTO_STRING_LIMIT {
            return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "response too big for into_string",
                    ));
        }

        #[cfg(feature = "charset")]
        {
            let (text, _, _) = encoding.decode(&buf);
            Ok(text.into_owned())
        }
        #[cfg(not(feature = "charset"))]
        {
            Ok(String::from_utf8_lossy(&buf).to_string())
        }
    }
    */

    /// Read the body of this response into a serde_json::Value, or any other type that
    /// implements the [serde::Deserialize] trait.
    ///
    /// You must use either a type annotation as shown below (`message: Message`), or the
    /// [turbofish operator] (`::<Type>`) so Rust knows what type you are trying to read.
    ///
    /// [turbofish operator]: https://matematikaadit.github.io/posts/rust-turbofish.html
    ///
    /// Requires feature `ureq = { version = "*", features = ["json"] }`
    ///
    /// Example:
    ///
    /// ```
    /// # fn main() -> Result<(), ureq::Error> {
    /// # ureq::is_test(true);
    /// use serde::{Deserialize, de::DeserializeOwned};
    ///
    /// #[derive(Deserialize)]
    /// struct Message {
    ///     hello: String,
    /// }
    ///
    /// let message: Message =
    ///     ureq::get("http://example.com/hello_world.json")
    ///         .call()?
    ///         .into_json()?;
    ///
    /// assert_eq!(message.hello, "world");
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// Or, if you don't want to define a struct to read your JSON into, you can
    /// use the convenient `serde_json::Value` type to parse arbitrary or unknown
    /// JSON.
    ///
    /// ```
    /// # fn main() -> Result<(), ureq::Error> {
    /// # ureq::is_test(true);
    /// let json: serde_json::Value = ureq::get("http://example.com/hello_world.json")
    ///     .call()?
    ///     .into_json()?;
    ///
    /// assert_eq!(json["hello"], "world");
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "json")]
    pub fn into_json<T: DeserializeOwned>(self) -> io::Result<T> {
        use crate::stream::io_err_timeout;
        use std::error::Error;

        let reader = self.into_reader();
        serde_json::from_reader(reader).map_err(|e| {
            // This is to unify TimedOut io::Error in the API.
            // We make a clone of the original error since serde_json::Error doesn't
            // let us get the wrapped error instance back.
            if let Some(ioe) = e.source().and_then(|s| s.downcast_ref::<io::Error>()) {
                if ioe.kind() == io::ErrorKind::TimedOut {
                    return io_err_timeout(ioe.to_string());
                }
            }

            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Failed to read JSON: {}", e),
                )
        })
    }

    /// Create a response from a Read trait impl.
    ///
    /// This is hopefully useful for unit tests.
    ///
    /// Example:
    ///
    /// use std::io::Cursor;
    ///
    /// let text = "HTTP/1.1 401 Authorization Required\r\n\r\nPlease log in\n";
    /// let read = Cursor::new(text.to_string().into_bytes());
    /// let resp = ureq::Response::do_from_read(read);
    ///
    /// assert_eq!(resp.status(), 401);
    pub(crate) fn do_from_stream(stream: Stream, unit: Unit) -> Result<Response, Error> {
        //
        // HTTP/1.1 200 OK\r\n
        //let mut stream = BufReader::with_capacity(4096, stream);
        let mut stream = stream;

        // The status line we can ignore non-utf8 chars and parse as_str_lossy().
        let (mut headers, carryover) = read_status_and_headers(&mut stream)?;

        let i = memchr::memchr(b'\n', &headers);
        if i.is_none() {
            return Err(ErrorKind::BadStatus.msg(""));
        }
        let i = i.unwrap();
        let status_line: StatusVec = headers.drain(..i+1).collect();
        //println!("Status: {}", std::str::from_utf8(&status_line).unwrap());

        //println!("Headers: {}", std::str::from_utf8(&headers).unwrap());
        let headers = Headers::try_from(headers)?;

        Ok(Response {
            status_line,
            headers,
            unit: unit,
            stream: stream,
            carryover: carryover,
            //history: HistoryVec::new(),
        })
    }

    pub(crate) fn do_from_request(unit: Unit, stream: Stream) -> Result<Response, Error> {
        let resp = Response::do_from_stream(stream, unit)?;
        Ok(resp)
    }

    #[cfg(test)]
    pub fn to_write_vec(self) -> Vec<u8> {
        self.stream.to_write_vec()
    }

    #[cfg(test)]
    pub fn set_url(&mut self, url: Url) {
        self.url = Some(url);
    }

    #[cfg(test)]
    pub fn history_from_previous(&mut self, previous: Response) {
        let previous_url = previous.get_url().to_string();
        self.history = previous.history;
        self.history.push(previous_url);
    }
}

// HTTP/1.1 200 OK\r\n
fn parse_status_line_from_header(s: &[u8]) -> Result<(&str, u16, &str), Error> {
    if s.len() < 12 || s[12] != b' ' || s[8] != b' ' {
        return Err(BadStatus.msg("Status line isn't formatted correctly"));
    }
    if s.iter().any(|c| !c.is_ascii()) {
        return Err(BadStatus.msg("Status line not ASCII"));
    }
    else if b"HTTP/1.1" != &s[..8] {
        return Err(BadStatus.msg("HTTP version not formatted correctly"));
    }
    else if s[9..12].iter().any(|c| !c.is_ascii_digit()) {
        return Err(BadStatus.msg("HTTP status code must be a 3 digit number"));
    }
    else {
        let n = std::str::from_utf8(&s[9..12]).unwrap();
        let status: u16 = n.parse().map_err(|_| BadStatus.new())?;

        let text = std::str::from_utf8(&s[12..]).unwrap();
        Ok((
            "HTTP/1.1",
            status,
            text,
            
        ))

    }
}

/*
impl FromStr for Response {
    type Err = Error;
    /// Parse a response from a string.
    ///
    /// Example:
    /// ```
    /// let s = "HTTP/1.1 200 OK\r\n\
    ///     X-Forwarded-For: 1.2.3.4\r\n\
    ///     Content-Type: text/plain\r\n\
    ///     \r\n\
    ///     Hello World!!!";
    /// let resp = s.parse::<ureq::Response>().unwrap();
    /// assert!(resp.has("X-Forwarded-For"));
    /// let body = resp.into_string().unwrap();
    /// assert_eq!(body, "Hello World!!!");
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let stream = Stream::from_vec(s.as_bytes().to_owned());
        Self::do_from_stream(stream, None)
    }
}
*/

fn read_status_and_headers(reader: &mut impl Read) -> io::Result<(BufVec, CarryOver)> {
    let mut buf = BufVec::new();
    let mut buffer = [0u8; 4096];

    let limited_reader = reader;
    //let mut limited_reader = reader.take(((MAX_HEADER_SIZE + 1) * MAX_HEADER_COUNT) as u64);

    let mut carry = 0;

    loop {
            let r = limited_reader.read(&mut buffer[carry..]);

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
                    //println!("Buffer state {}", std::str::from_utf8(&buffer[..(c - i - 4)]).unwrap());
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

    //println!("Header segment size: {}", std::str::from_utf8(&buffer).unwrap());

    let mut carryover = CarryOver::new();
    let _ = carryover.try_extend_from_slice(&buffer[..carry]).unwrap();
    Ok((buf.into(), carryover))
}

/// Limits a `Read` to a content size (as set by a "Content-Length" header).
struct LimitedRead<R> {
    reader: R,
    limit: usize,
    position: usize,
}

impl<R: Read> LimitedRead<R> {
    fn new(reader: R, limit: usize) -> Self {
        LimitedRead {
            reader,
            limit,
            position: 0,
        }
    }
}

impl<R: Read> Read for LimitedRead<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.limit - self.position;
        if left == 0 {
            return Ok(0);
        }
        let from = if left < buf.len() {
            &mut buf[0..left]
        } else {
            buf
        };
        match self.reader.read(from) {
            // https://tools.ietf.org/html/rfc7230#page-33
            // If the sender closes the connection or
            // the recipient times out before the indicated number of octets are
            // received, the recipient MUST consider the message to be
            // incomplete and close the connection.
            Ok(0) => Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "response body closed before all bytes were read",
                    )),
            Ok(amount) => {
                self.position += amount;
                Ok(amount)
            }
            Err(e) => Err(e),
        }
    }
}

impl<R: Read> From<LimitedRead<R>> for Stream
where
Stream: From<R>,
{
    fn from(limited_read: LimitedRead<R>) -> Stream {
        limited_read.reader.into()
    }
}

/// Extract the charset from a "Content-Type" header.
///
/// "Content-Type: text/plain; charset=iso8859-1" -> "iso8859-1"
///
/// *Internal API*
pub(crate) fn charset_from_content_type(header: Option<&str>) -> &str {
    header
        .and_then(|header| {
            header.find(';').and_then(|semi| {
                (&header[semi + 1..])
                    .find('=')
                    .map(|equal| (&header[semi + equal + 2..]).trim())
            })
        })
    .unwrap_or(DEFAULT_CHARACTER_SET)
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

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn short_read() {
        use std::io::Cursor;
        let mut lr = LimitedRead::new(Cursor::new(vec![b'a'; 3]), 10);
        let mut buf = vec![0; 1000];
        let result = lr.read_to_end(&mut buf);
        assert!(result.err().unwrap().kind() == io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn content_type_without_charset() {
        let s = "HTTP/1.1 200 OK\r\n\
                 Content-Type: application/json\r\n\
                 \r\n\
                 OK";
        let resp = s.parse::<Response>().unwrap();
        assert_eq!("application/json", resp.content_type());
    }

    #[test]
    fn content_type_without_cr() {
        let s = "HTTP/1.1 200 OK\r\n\
                 Content-Type: application/json\n\
                 \r\n\
                 OK";
        let resp = s.parse::<Response>().unwrap();
        assert_eq!("application/json", resp.content_type());
    }

    #[test]
    fn content_type_with_charset() {
        let s = "HTTP/1.1 200 OK\r\n\
                 Content-Type: application/json; charset=iso-8859-4\r\n\
                 \r\n\
                 OK";
        let resp = s.parse::<Response>().unwrap();
        assert_eq!("application/json", resp.content_type());
    }

    #[test]
    fn content_type_default() {
        let s = "HTTP/1.1 200 OK\r\n\r\nOK";
        let resp = s.parse::<Response>().unwrap();
        assert_eq!("text/plain", resp.content_type());
    }

    #[test]
    fn charset() {
        let s = "HTTP/1.1 200 OK\r\n\
                 Content-Type: application/json; charset=iso-8859-4\r\n\
                 \r\n\
                 OK";
        let resp = s.parse::<Response>().unwrap();
        assert_eq!("iso-8859-4", resp.charset());
    }

    #[test]
    fn charset_default() {
        let s = "HTTP/1.1 200 OK\r\n\
                 Content-Type: application/json\r\n\
                 \r\n\
                 OK";
        let resp = s.parse::<Response>().unwrap();
        assert_eq!("utf-8", resp.charset());
    }

    #[test]
    fn chunked_transfer() {
        let s = "HTTP/1.1 200 OK\r\n\
                 Transfer-Encoding: Chunked\r\n\
                 \r\n\
                 3\r\n\
                 hel\r\n\
                 b\r\n\
                 lo world!!!\r\n\
                 0\r\n\
                 \r\n";
        let resp = s.parse::<Response>().unwrap();
        assert_eq!("hello world!!!", resp.into_string().unwrap());
    }

    #[test]
    fn into_string_large() {
        const LEN: usize = INTO_STRING_LIMIT + 1;
        let s = format!(
            "HTTP/1.1 200 OK\r\n\
                 Content-Length: {}\r\n
                 \r\n
                 {}",
                 LEN,
                 "A".repeat(LEN),
                 );
        let result = s.parse::<Response>().unwrap();
        let err = result
            .into_string()
            .expect_err("didn't error with too-long body");
        assert_eq!(err.to_string(), "response too big for into_string");
        assert_eq!(err.kind(), io::ErrorKind::Other);
    }

    #[test]
    #[cfg(feature = "json")]
    fn parse_simple_json() {
        let s = "HTTP/1.1 200 OK\r\n\
             \r\n\
             {\"hello\":\"world\"}";
        let resp = s.parse::<Response>().unwrap();
        let v: serde_json::Value = resp.into_json().unwrap();
        let compare = "{\"hello\":\"world\"}"
            .parse::<serde_json::Value>()
            .unwrap();
        assert_eq!(v, compare);
    }

    #[test]
    #[cfg(feature = "json")]
    fn parse_deserialize_json() {
        use serde::Deserialize;

        #[derive(Deserialize)]
        struct Hello {
            hello: String,
        }

        let s = "HTTP/1.1 200 OK\r\n\
             \r\n\
             {\"hello\":\"world\"}";
        let resp = s.parse::<Response>().unwrap();
        let v: Hello = resp.into_json::<Hello>().unwrap();
        assert_eq!(v.hello, "world");
    }

    #[test]
    fn parse_borked_header() {
        let s = "HTTP/1.1 BORKED\r\n".to_string();
        let err = s.parse::<Response>().unwrap_err();
        assert_eq!(err.kind(), BadStatus);
    }

    #[test]
    fn parse_header_without_reason() {
        let s = "HTTP/1.1 302\r\n\r\n".to_string();
        let resp = s.parse::<Response>().unwrap();
        assert_eq!(resp.status_text(), "");
    }

    #[test]
    fn read_next_line_large() {
        const LEN: usize = MAX_HEADER_SIZE + 1;
        let s = format!("Long-Header: {}\r\n", "A".repeat(LEN),);
        let mut cursor = Cursor::new(s);
        let result = read_next_line(&mut cursor, "some context");
        let err = result.expect_err("did not error on too-large header");
        assert_eq!(err.kind(), io::ErrorKind::Other);
        assert_eq!(
            err.to_string(),
            format!("header field longer than {} bytes", MAX_HEADER_SIZE)
            );
    }

    #[test]
    fn too_many_headers() {
        const LEN: usize = MAX_HEADER_COUNT + 1;
        let s = format!(
            "HTTP/1.1 200 OK\r\n\
                 {}
                 \r\n
                 hi",
                 "Header: value\r\n".repeat(LEN),
                 );
        let err = s
            .parse::<Response>()
            .expect_err("did not error on too many headers");
        assert_eq!(err.kind(), ErrorKind::BadHeader);
        assert_eq!(
            err.to_string(),
            format!(
                "Bad Header: more than {} header fields in response",
                MAX_HEADER_COUNT
                )
            );
    }

    #[test]
    #[cfg(feature = "charset")]
    fn read_next_line_non_ascii_reason() {
        let (cow, _, _) =
            encoding_rs::WINDOWS_1252.encode("HTTP/1.1 302 Déplacé Temporairement\r\n");
        let bytes = cow.to_vec();
        let mut reader = io::BufReader::new(io::Cursor::new(bytes));
        let r = read_next_line(&mut reader, "test status line");
        let h = r.unwrap();
        assert_eq!(h.to_string(), "HTTP/1.1 302 D�plac� Temporairement");
    }

    #[test]
    #[cfg(feature = "charset")]
    fn parse_header_with_non_utf8() {
        let (cow, _, _) = encoding_rs::WINDOWS_1252.encode(
            "HTTP/1.1 200 OK\r\n\
            x-geo-header: gött mos!\r\n\
            \r\n\
            OK",
            );
        let v = cow.to_vec();
        let s = Stream::from_vec(v);
        let resp = Response::do_from_stream(s.into(), None).unwrap();
        assert_eq!(resp.status(), 200);
        assert_eq!(resp.header("x-geo-header"), None);
    }

    #[test]
    fn history() {
        let mut response0 = Response::new(302, "Found", "").unwrap();
        response0.set_url("http://1.example.com/".parse().unwrap());
        assert!(response0.history.is_empty());

        let mut response1 = Response::new(302, "Found", "").unwrap();
        response1.set_url("http://2.example.com/".parse().unwrap());
        response1.history_from_previous(response0);

        let mut response2 = Response::new(404, "NotFound", "").unwrap();
        response2.set_url("http://2.example.com/".parse().unwrap());
        response2.history_from_previous(response1);

        let hist: Vec<&str> = response2.history.iter().map(|r| &**r).collect();
        assert_eq!(hist, ["http://1.example.com/", "http://2.example.com/"])
    }

    #[test]
    fn response_implements_send_and_sync() {
        let _response: Box<dyn Send> = Box::new(Response::new(302, "Found", "").unwrap());
        let _response: Box<dyn Sync> = Box::new(Response::new(302, "Found", "").unwrap());
    }

    #[test]
    fn ensure_response_size() {
        // This is platform dependent, so we can't be too strict or precise.
        let size = std::mem::size_of::<Response>();
        println!("Response size: {}", size);
        assert!(size < 400); // 200 on Macbook M1
    }
}
