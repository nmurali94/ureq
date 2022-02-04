use crate::error::{Error, ErrorKind};
use std::fmt;
use std::str::{from_utf8, FromStr};
use std::convert::TryFrom;

const MAX_HEADER_SIZE: usize = 1_024;
const MAX_HEADER_COUNT: usize = 128;

type HeaderName = arrayvec::ArrayString<128>;
type HeaderValue = arrayvec::ArrayVec<u8, 896>;

use std::collections::BTreeMap;

pub struct Headers(BTreeMap<HeaderName, HeaderValue>);

impl <const N: usize> TryFrom<arrayvec::ArrayVec<u8, N>> for Headers {
    type Error = Error;
    fn try_from(v: arrayvec::ArrayVec<u8, N>) -> Result<Self, Error> {
        let mut start = 0;
        let mut map = BTreeMap::new();
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
            let data = &v[start..colon];
            let data = data.iter()
                .map(|c| c.to_ascii_lowercase())
                .collect::<arrayvec::ArrayVec<u8, 128>>();
            let key = std::str::from_utf8(&data);
            if key.is_err() {
                return Err(ErrorKind::BadHeader.msg("HTTP header name must be a ascii"));
            }
            let key = HeaderName::try_from(key.unwrap().trim()).unwrap();

            let value = HeaderValue::try_from(&v[colon+1..end]).unwrap();

            start = n + 1;
            map.insert(key, value);
            if map.len() > MAX_HEADER_COUNT {
                return Err(ErrorKind::BadHeader.msg("HTTP header count exceeds the max supported"));
            }
        }
        Ok(Headers(map))
    }
}

impl Headers {
    pub fn header(&self, name: &str) -> Option<&[u8]> {
        let data = name.trim().bytes()
            .map(|c| c.to_ascii_lowercase())
            .collect::<arrayvec::ArrayVec<u8, 128>>();
        let key = std::str::from_utf8(&data).unwrap();
        self.0.get(key).map(|v| v.as_ref())
    }
}

