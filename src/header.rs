use crate::error::{Error, ErrorKind};
use std::convert::TryFrom;

#[derive(Clone, Copy)]
struct Header {
    meta: usize, // 0x00, 0x00, 0xcolon, 0xlen
    data: [u8; 1024],
}

pub struct Headers{
    len: usize,
    arr: [Header; 64],
}

impl Headers {
    const fn new() -> Self {
        Headers {
            len: 0, arr: [Header{ meta: 0, data: [0; 1024] }; 64]
        }
    }

    fn push(&mut self, t: Header) {
        self.arr[self.len] = t;
        self.len += 1;
    }
}

impl TryFrom<&[u8]> for Headers {
    type Error = Error;
    fn try_from(v: &[u8]) -> Result<Self, Error> {
        let mut map = Headers::new();
        let mut start = 0;
        while let Some(len) = v[start..].windows(2).position(|x| x == b"\r\n") {
            if len > 1024 {
                return Err(ErrorKind::BadHeader.msg("HTTP header size larger than supported"));
            }
            let colon = &v[start..start+len].iter().position(|x| *x == b':').ok_or_else(|| {
                ErrorKind::BadHeader.msg("HTTP header must be a key-value separated by a colon")
            })?;
            let mut data = [0; 1024];
            data[..len].copy_from_slice(&v[start..start+len]);

            let meta = ((colon & 0xFFFF) << 16) | (len & 0xFFFF); 
            let h = Header {
                meta,
                data,
            };
            map.push(h);
            start += len + 2;
        }
        Ok(map)
    }
}

impl Headers {
    pub fn header(&self, name: &str) -> Option<&[u8]> {
        for header in &self.arr[..self.len] {
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
