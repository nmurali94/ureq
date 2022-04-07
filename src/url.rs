
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
    #[cfg(feature="tls")]
    Https,
}

impl Scheme {
    fn to_str(self) -> &'static str {
        use Scheme::*;
        match self {
            Http => "http",
            #[cfg(feature="tls")]
            Https => "https",
        }
    }
}

impl Url {
    pub fn parse(s: &str) -> Result<Self, Error> {

        if s.len() > 256 {
            return Err(Error::UnsupportedLength);
        }
        if !s.is_ascii() {
            return Err(Error::Ascii);
        }

        let bs = s.as_bytes();
        let si = memchr::memmem::find(bs, b"://").ok_or(Error::Scheme)?;
        let scheme = match &bs[..si] {
            b"http" => Ok(Scheme::Http),
            #[cfg(feature="tls")]
            b"https" => Ok(Scheme::Https),
            _ => Err(Error::Scheme),
        }?;
        let hi = si + 3;

        let hj = memchr::memchr(b'/', &bs[hi..]).ok_or(Error::Host)?;
        let hj = hi + hj;
        let pk = memchr::memchr(b':', &bs[hi..hj]);
        let v = match scheme {
            Scheme::Http => 80,
            #[cfg(feature="tls")]
            Scheme::Https => 443,
        };
        let port = pk.and_then(|k| (&s[hi + k..hj]).parse::<u16>().ok()).unwrap_or(v);

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
