use std::fmt;
use std::io::{empty, Cursor, Read};

#[cfg(feature = "charset")]
use crate::response::DEFAULT_CHARACTER_SET;
#[cfg(feature = "charset")]
use encoding_rs::Encoding;

#[cfg(feature = "json")]
use super::SerdeValue;

/// The different kinds of bodies to send.
///
/// *Internal API*
#[allow(dead_code)]
pub(crate) enum Payload<'a> {
    Empty,
    Text(&'a str, String),
    #[cfg(feature = "json")]
    JSON(SerdeValue),
    Reader(Box<dyn Read + 'a>),
    Bytes(&'a [u8]),
}

impl fmt::Debug for Payload<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Payload::Empty => write!(f, "Empty"),
            Payload::Text(t, _) => write!(f, "{}", t),
            #[cfg(feature = "json")]
            Payload::JSON(_) => write!(f, "JSON"),
            Payload::Reader(_) => write!(f, "Reader"),
            Payload::Bytes(v) => write!(f, "{:?}", v),
        }
    }
}

impl Default for Payload<'_> {
    fn default() -> Self {
        Payload::Empty
    }
}

/// The size of the body.
///
/// *Internal API*
#[derive(Debug)]
pub(crate) enum BodySize {
    Empty,
    Unknown,
    Known(u64),
}

/// Payloads are turned into this type where we can hold both a size and the reader.
///
/// *Internal API*
pub(crate) struct SizedReader<'a> {
    pub size: BodySize,
    pub reader: Box<dyn Read + 'a>,
}

impl fmt::Debug for SizedReader<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "SizedReader[size={:?},reader]", self.size)
    }
}

impl<'a> SizedReader<'a> {
    fn new(size: BodySize, reader: Box<dyn Read + 'a>) -> Self {
        SizedReader { size, reader }
    }
}

impl<'a> Payload<'a> {
    pub fn into_read(self) -> SizedReader<'a> {
        match self {
            Payload::Empty => SizedReader::new(BodySize::Empty, Box::new(empty())),
            Payload::Text(text, _charset) => {
                #[cfg(feature = "charset")]
                let bytes = {
                    let encoding = Encoding::for_label(_charset.as_bytes())
                        .or_else(|| Encoding::for_label(DEFAULT_CHARACTER_SET.as_bytes()))
                        .unwrap();
                    encoding.encode(text).0
                };
                #[cfg(not(feature = "charset"))]
                let bytes = text.as_bytes();
                let len = bytes.len();
                let cursor = Cursor::new(bytes);
                SizedReader::new(BodySize::Known(len as u64), Box::new(cursor))
            }
            #[cfg(feature = "json")]
            Payload::JSON(v) => {
                let bytes = serde_json::to_vec(&v).expect("Bad JSON in payload");
                let len = bytes.len();
                let cursor = Cursor::new(bytes);
                SizedReader::new(BodySize::Known(len as u64), Box::new(cursor))
            }
            Payload::Reader(read) => SizedReader::new(BodySize::Unknown, read),
            Payload::Bytes(bytes) => {
                let len = bytes.len();
                let cursor = Cursor::new(bytes);
                SizedReader::new(BodySize::Known(len as u64), Box::new(cursor))
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_copy_chunked() {
        let mut source = Vec::<u8>::new();
        source.resize(CHUNK_MAX_PAYLOAD_SIZE, 33);
        source.extend_from_slice(b"hello world");

        let mut dest = Vec::<u8>::new();
        copy_chunked(&mut &source[..], &mut dest).unwrap();

        let mut dest_expected = Vec::<u8>::new();
        dest_expected.extend_from_slice(format!("{:x}\r\n", CHUNK_MAX_PAYLOAD_SIZE).as_bytes());
        dest_expected.resize(dest_expected.len() + CHUNK_MAX_PAYLOAD_SIZE, 33);
        dest_expected.extend_from_slice(b"\r\n");

        dest_expected.extend_from_slice(b"b\r\nhello world\r\n");
        dest_expected.extend_from_slice(b"0\r\n\r\n");

        assert_eq!(dest, dest_expected);
    }
}
