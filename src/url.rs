use crate::error::Error as UreqError;
use std::error::Error as StdError;
use std::fmt;

#[derive(Debug)]
pub struct Url {
    serialization: String,
    scheme: Scheme,
    meta: u64, // 0x0000 0xhost 0xport 0xpath
}

#[derive(Debug)]
pub enum Error {
    UnsupportedLength,
    Ascii,
    Scheme,
    Path,
    Host,
}

#[derive(Copy, Clone, Debug)]
pub enum Scheme {
    Http,
    #[cfg(feature = "tls")]
    Https,
}

impl Scheme {
    fn _to_str(self) -> &'static str {
        use Scheme::*;
        match self {
            Http => "http",
            #[cfg(feature = "tls")]
            Https => "https",
        }
    }
}

impl Url {
    pub fn parse(s: &str) -> Result<Self, UreqError> {
        if s.is_empty() || s.len() > 256 {
            return Err(UreqError::from(Error::UnsupportedLength));
        }
        if !s.is_ascii() {
            return Err(UreqError::from(Error::Ascii));
        }

        let bs = s.as_bytes();
        let si = bs.windows(3).position(|window| window == b"://")
            .ok_or_else(|| UreqError::from(Error::Scheme))?;
        let scheme = match &bs[..si] {
            b"http" => Ok(Scheme::Http),
            #[cfg(feature = "tls")]
            b"https" => Ok(Scheme::Https),
            _ => Err(UreqError::from(Error::Scheme)),
        }?;
        let hi = si + 3;

        let hj = &bs[hi..].iter().position(|x| *x == b'/')
            .ok_or_else(|| UreqError::from(Error::Host))?;
        let hj = hi + hj;
        let pk = &bs[hi..hj].iter().position(|x| *x == b':');
        let v = match scheme {
            Scheme::Http => 80,
            #[cfg(feature = "tls")]
            Scheme::Https => 443,
        };
        let port = pk
            .and_then(|k| (&s[hi + k..hj]).parse::<u16>().ok())
            .unwrap_or(v);

        let hi = hi as u8;
        let l = pk.unwrap_or(hj) as u8;

        let i = hj as u8;
        let j = bs.len() as u8;

        let ho = ((hi as u64) << 8) | l as u64;
        let pa = ((i as u64) << 8) | j as u64;

        let meta = (ho << 32) | ((port as u64) << 16) | pa;

        let url = Url {
            serialization: s.to_string(),
            scheme,
            meta,
        };

        Ok(url)
    }
    pub fn serialization(&self) -> &str {
        self.serialization.as_str()
    }

    pub fn host_str(&self) -> &str {
        let m = (self.meta >> 32) & 0x0000FFFF;
        let i = ((m & 0xFF00) >> 8) as usize;
        let j = (m & 0x00FF) as usize;
        &self.serialization[i..j]
    }

    pub fn scheme(&self) -> Scheme {
        self.scheme
    }

    pub fn path(&self) -> &str {
        let m = self.meta & 0x0000FFFF;
        let i = ((m & 0xFF00) >> 8) as usize;
        let j = (m & 0x00FF) as usize;
        &self.serialization[i..j]
    }

    pub fn port(&self) -> u16 {
        (((self.meta) << 32) >> 48) as u16
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl StdError for Error {}
