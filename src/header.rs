use crate::error::{Error, ErrorKind};
use std::convert::TryFrom;

const MAX_HEADER_SIZE: usize = 1_024;
const MAX_HEADER_COUNT: usize = 32;

type HeaderName = arrayvec::ArrayVec<u8, 128>;
type HeaderValue = arrayvec::ArrayVec<u8, 896>;

use std::collections::BTreeMap;

pub struct Headers(BTreeMap<HeaderName, HeaderValue>);

impl <const N: usize> TryFrom<arrayvec::ArrayVec<u8, N>> for Headers {
    type Error = Error;
    fn try_from(v: arrayvec::ArrayVec<u8, N>) -> Result<Self, Error> {
        let mut start = 0;
        let mut map = BTreeMap::new();
        for end in memchr::memmem::find(&v, b"\r\n") {
            let colon = memchr::memchr(b':', &v[start..end]).ok_or(ErrorKind::BadHeader.msg("HTTP header must be a key-value separated by a colon"))?;
            let data = &v[start..colon];
            let key = data.iter()
                .map(|c| c.to_ascii_lowercase())
                .collect::<HeaderName>();
			let value = (&v[colon+1..end]).iter().copied()
				.collect::<HeaderValue>();
            start = end + 2;
            map.insert(key, value);
        }
        Ok(Headers(map))
    }
}

impl Headers {
    pub fn header(&self, name: &str) -> Option<&[u8]> {
        let key = name.trim().bytes()
            .map(|c| c.to_ascii_lowercase())
            .collect::<HeaderName>();
        self.0.get(&key).map(|v| v.as_ref())
    }
}

