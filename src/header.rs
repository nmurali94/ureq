use crate::error::{Error, ErrorKind};
use std::convert::TryFrom;

type HeaderVec = Vec<Header>;

struct Header {
    meta: usize, // 0x00, 0x00, 0xcolon, 0xlen
    data: [u8; 1024],
}

pub struct Headers(HeaderVec);

impl TryFrom<Vec<u8>> for Headers {
    type Error = Error;
    fn try_from(v: Vec<u8>) -> Result<Self, Error> {
        let mut start = 0;
        let mut map = HeaderVec::new();
        for end in memchr::memmem::find_iter(&v, b"\r\n") {
            if end - start > 1024 {
                return Err(ErrorKind::BadHeader.msg("HTTP header size larger than supported"));
            }
            let colon = memchr::memchr(b':', &v[start..end]).ok_or_else(|| {
                ErrorKind::BadHeader.msg("HTTP header must be a key-value separated by a colon")
            })?;
            let mut data = [0; 1024];
            data[..(end - start)].copy_from_slice(&v[start..end]);


            let meta = ((colon & 0xFFFF) << 16) | ((end - start) & 0xFFFF); 
            let h = Header {
                meta,
                data,
            };
            map.push(h);
            start = end + 2;
        }
        Ok(Headers(map))
    }
}

impl Headers {
    pub fn header(&self, name: &str) -> Option<&[u8]> {
        for header in &self.0 {
            let meta = &header.meta;
            let len = meta & 0xFFFF;
            let colon = (meta >> 16) & 0xFFFF;

            let data_key = &header.data[..colon];
            let v = &header.data[colon + 1..len];
            if eq(name.trim().as_bytes(), data_key) {
                return Some(v);
            }
        }
        None
    }
}

fn eq(given: &[u8], stored: &[u8]) -> bool {
    if given.len() != stored.len() {
        return false;
    }
    for i in 0..given.len() {
        let g = given[i].to_ascii_lowercase();
        let s = stored[i].to_ascii_lowercase();
        if g != s {
            return false;
        };
    }
    true
}
