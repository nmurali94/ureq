use crate::error::{Error, ErrorKind};
use std::fmt;
use std::str::{from_utf8, FromStr};
use std::convert::TryFrom;

const MAX_HEADER_SIZE: usize = 1_024;
const MAX_HEADER_COUNT: usize = 128;

type HeaderVec = tinyvec::TinyVec<[HeaderIndex; MAX_HEADER_COUNT]>;
type HeaderLineVec = tinyvec::TinyVec<[u8; MAX_HEADER_SIZE]>;

pub struct Headers {
    indices: HeaderVec,
    data: Vec<u8>,
}

struct HeaderIndex {
    start: usize,
    colon: usize,
    end: usize,
}

impl Default for HeaderIndex {
    fn default() -> Self {
        HeaderIndex {
            start: 0,
            colon: 0,
            end: 0,
        }
    }
}

impl TryFrom<Vec<u8>> for Headers {
    type Error = Error;
    fn try_from(v: Vec<u8>) -> Result<Self, Error> {
        let mut start = 0;
        let mut hs = HeaderVec::new();
        for n in memchr::memchr_iter(b'\n', &v) {
            let end = if v[n-1] == b'\r' {
                n-1
            } else { n };
            let c = memchr::memchr(b':', &v[start..end]);
            if c.is_none() {
                return Err(ErrorKind::BadHeader.msg("HTTP header must be a key-value separated by a colon"));

            }
            if end - start > MAX_HEADER_SIZE {
                return Err(ErrorKind::BadHeader.msg("HTTP header size exceeds the max supported"));

            }
            let colon = start + c.unwrap();

            let h = HeaderIndex {
                start,
                colon,
                end,
            };

            start = n + 1;
            hs.push(h);
        }
        Ok(Headers {
            indices: hs,
            data: v
        })
    }
}

impl Headers {
    pub fn header(&self, name: &str) -> Option<&[u8]> {
        for idx in &self.indices {
            let h = &self.data[idx.start..idx.colon];
            let h_str = std::str::from_utf8(h).unwrap();
            if h_str.to_lowercase() == name.to_lowercase() {
                let v = &self.data[idx.colon+1..idx.end];
                return Some(v);
            }
        }
        None
    }
}

/// Since a status line or header can contain non-utf8 characters the
/// backing store is a `Vec<u8>`
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HeaderLine(HeaderLineVec);

impl From<HeaderLineVec> for HeaderLine {
    fn from(b: HeaderLineVec) -> Self {
        HeaderLine(b)
    }
}

impl HeaderLine {

    fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn into_header(self) -> Result<Header, Error> {
        // The header name should always be ascii, we can read anything up to the
        // ':' delimiter byte-by-byte.
        let mut index = 0;

        for c in self.as_bytes() {
            if *c == b':' {
                break;
            }
            if !is_tchar(c) {
                return Err(Error::new(
                    ErrorKind::BadHeader,
                    Some(format!("Invalid char ({:0x?}) while looking for ':'", *c)),
                ));
            }
            index += 1;
        }

        Ok(Header { line: self, index })
    }
}

impl fmt::Display for HeaderLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", String::from_utf8_lossy(&self.0))
    }
}

#[derive(Clone, PartialEq)]
/// Wrapper type for a header field.
/// https://tools.ietf.org/html/rfc7230#section-3.2
pub struct Header {
    // Line contains the unmodified bytes of single header field.
    // It does not contain the final CRLF.
    line: HeaderLine,
    // Index is the position of the colon within the header field.
    // Invariant: index > 0
    // Invariant: index + 1 < line.len()
    index: usize,
}

impl Default for Header {
    fn default() -> Self {
        Header { line: HeaderLine(HeaderLineVec::new()), index: 0 }
    }
}

impl fmt::Debug for Header {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.line)
    }
}

impl Header {
    pub fn new(name: &str, value: &str) -> Self {
        let mut line = HeaderLineVec::new();
        line.extend_from_slice(name.as_bytes());
        line.extend_from_slice(b": ");
        line.extend_from_slice(value.as_bytes());
        let index = name.len();

        let line = HeaderLine(line);

        Header { line, index }
    }

    /// The header name.
    pub fn name(&self) -> &str {
        let bytes = &self.line.as_bytes()[0..self.index];
        // Since we validate the header name in HeaderLine::into_header, we
        // are guaranteed it is valid utf-8 at this point.
        from_utf8(bytes).expect("Legal chars in header name")
    }

    /// The header value.
    ///
    /// For non-utf8 headers this returns None (use [`Header::value_raw()`]).
    pub fn value(&self) -> Option<&str> {
        let bytes = &self.line.as_bytes()[self.index + 1..];
        from_utf8(bytes)
            .map(|s| s.trim())
            .ok()
            // ensure all bytes are valid field name.
            .filter(|s| s.as_bytes().iter().all(is_field_vchar_or_obs_fold))
    }

    /// The header value as a byte slice.
    ///
    /// For legacy reasons, the HTTP spec allows headers to be non-ascii characters.
    /// Typically such headers are encoded in a non-utf8 encoding (such as iso-8859-1).
    ///
    /// ureq can't know what encoding the header is in, but this function provides
    /// an escape hatch for users that need to handle such headers.
    pub fn value_raw(&self) -> &[u8] {
        let mut bytes = &self.line.as_bytes()[self.index + 1..];

        if !bytes.is_empty() {
            // trim front
            while !bytes.is_empty() && bytes[0].is_ascii_whitespace() {
                bytes = &bytes[1..];
            }
            // trim back
            while !bytes.is_empty() && bytes[bytes.len() - 1].is_ascii_whitespace() {
                bytes = &bytes[..(bytes.len() - 1)];
            }
        }

        bytes
    }

    /// Compares the given str to the header name ignoring case.
    pub fn is_name(&self, other: &str) -> bool {
        self.name().eq_ignore_ascii_case(other)
    }

    pub(crate) fn validate(&self) -> Result<(), Error> {
        let bytes = self.line.as_bytes();
        let name_raw = &bytes[0..self.index];
        let value_raw = &bytes[self.index + 1..];

        if !valid_name(name_raw) || !valid_value(value_raw) {
            Err(ErrorKind::BadHeader.msg(&format!("invalid header '{}'", self.line)))
        } else {
            Ok(())
        }
    }
}

pub fn get_header<'a, 'b>(headers: &'b [Header], name: &'a str) -> Option<&'b str> {
    headers
        .iter()
        .find(|h| h.is_name(name))
        .and_then(|h| h.value())
}

pub fn has_header(headers: &[Header], name: &str) -> bool {
    get_header(headers, name).is_some()
}

// https://tools.ietf.org/html/rfc7230#section-3.2
// Each header field consists of a case-insensitive field name followed
// by a colon (":"), optional leading whitespace, the field value, and
// optional trailing whitespace.
// field-name     = token
// token = 1*tchar
// tchar = "!" / "#" / "$" / "%" / "&" / "'" / "*" / "+" / "-" / "." /
// "^" / "_" / "`" / "|" / "~" / DIGIT / ALPHA
fn valid_name(name: &[u8]) -> bool {
    !name.is_empty() && name.iter().all(is_tchar)
}

#[inline]
pub(crate) fn is_tchar(b: &u8) -> bool {
    match b {
        b'!' | b'#' | b'$' | b'%' | b'&' => true,
        b'\'' | b'*' | b'+' | b'-' | b'.' => true,
        b'^' | b'_' | b'`' | b'|' | b'~' => true,
        b if b.is_ascii_alphanumeric() => true,
        _ => false,
    }
}

// https://tools.ietf.org/html/rfc7230#section-3.2
// Note that field-content has an errata:
// https://www.rfc-editor.org/errata/eid4189
// field-value    = *( field-content / obs-fold )
// field-content  = field-vchar [ 1*( SP / HTAB ) field-vchar ]
// field-vchar    = VCHAR / obs-text
//
// obs-fold       = CRLF 1*( SP / HTAB )
//               ; obsolete line folding
//               ; see Section 3.2.4
// https://tools.ietf.org/html/rfc5234#appendix-B.1
// VCHAR          =  %x21-7E
//                        ; visible (printing) characters
fn valid_value(value: &[u8]) -> bool {
    value.iter().all(is_field_vchar_or_obs_fold)
}

#[inline]
fn is_field_vchar_or_obs_fold(b: &u8) -> bool {
    match b {
        b' ' | b'\t' => true,
        0x21..=0x7E => true,
        _ => false,
    }
}

impl FromStr for Header {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        //
        let mut line = HeaderLineVec::new();
        line.extend_from_slice(s.as_bytes());

        let line = HeaderLine(line);
        let header = line.into_header()?;

        header.validate()?;
        Ok(header)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_name() {
        assert!(valid_name(b"example"));
        assert!(valid_name(b"Content-Type"));
        assert!(valid_name(b"h-123456789"));
        assert!(!valid_name(b"Content-Type:"));
        assert!(!valid_name(b"Content-Type "));
        assert!(!valid_name(b" some-header"));
        assert!(!valid_name(b"\"invalid\""));
        assert!(!valid_name(b"G\xf6del"));
    }

    #[test]
    fn test_valid_value() {
        assert!(valid_value(b"example"));
        assert!(valid_value(b"foo bar"));
        assert!(valid_value(b" foobar "));
        assert!(valid_value(b" foo\tbar "));
        assert!(valid_value(b" foo~"));
        assert!(valid_value(b" !bar"));
        assert!(valid_value(b" "));
        assert!(!valid_value(b" \nfoo"));
        assert!(!valid_value(b"foo\x7F"));
    }

    #[test]
    fn test_parse_invalid_name() {
        let cases = vec![
            "Content-Type  :",
            " Content-Type: foo",
            "Content-Type foo",
            "\"some-header\": foo",
            "Gödel: Escher, Bach",
            "Foo: \n",
            "Foo: \nbar",
            "Foo: \x7F bar",
        ];
        for c in cases {
            let result = c.parse::<Header>();
            assert!(
                matches!(result, Err(ref e) if e.kind() == ErrorKind::BadHeader),
                "'{}'.parse(): expected BadHeader, got {:?}",
                c,
                result
            );
        }
    }

    #[test]
    #[cfg(feature = "charset")]
    fn test_parse_non_utf8_value() {
        let (cow, _, _) = encoding_rs::WINDOWS_1252.encode("x-geo-stuff: älvsjö ");
        let bytes = cow.to_vec();
        let line: HeaderLine = bytes.into();
        let header = line.into_header().unwrap();
        assert_eq!(header.name(), "x-geo-stuff");
        assert_eq!(header.value(), None);
        assert_eq!(header.value_raw(), [228, 108, 118, 115, 106, 246]);
    }

    #[test]
    fn empty_value() {
        let h = "foo:".parse::<Header>().unwrap();
        assert_eq!(h.value(), Some(""));
    }

    #[test]
    fn value_with_whitespace() {
        let h = "foo:      bar    ".parse::<Header>().unwrap();
        assert_eq!(h.value(), Some("bar"));
    }

    #[test]
    fn name_and_value() {
        let header: Header = "X-Forwarded-For: 127.0.0.1".parse().unwrap();
        assert_eq!("X-Forwarded-For", header.name());
        assert_eq!(header.value(), Some("127.0.0.1"));
        assert!(header.is_name("X-Forwarded-For"));
        assert!(header.is_name("x-forwarded-for"));
        assert!(header.is_name("X-FORWARDED-FOR"));
    }

    #[test]
    fn test_iso8859_utf8_mixup() {
        // C2 A5 is ¥ in UTF-8 and Â¥ in ISO-8859-1
        let b = "header: \0xc2\0xa5".to_string().into_bytes();
        let l: HeaderLine = b.into();
        let h = l.into_header().unwrap();
        assert_eq!(h.value(), None);
    }
}
