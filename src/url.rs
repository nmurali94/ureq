
#[derive(Debug, Clone)]
pub struct Url {
    serialization: String,
    scheme: (usize, usize),
    host: (usize, usize), 
    port: Option<u16>,
    path: (usize, usize),
}

#[derive(Debug, Clone)]
pub enum ParseError {
    AsciiError,
    SchemeError,
    PathError,
    HostError,
}

impl Url {
    pub fn parse(s: String) -> Result<Self, ParseError> {
        if !s.is_ascii() {
            return Err(ParseError::AsciiError);
        }

        let bs = s.as_bytes();
        let si = memchr::memmem::find(bs, b"://");
        if si.is_none() {
            return Err(ParseError::SchemeError);
        }
        let si = si.unwrap();
        let scheme = (0,si);
        let hi = si + 3;

        let hj = memchr::memchr(b'/', &bs[hi..]);
        if hj.is_none() {
            return Err(ParseError::HostError);
        }
        let hj = hi + hj.unwrap();
        let pk = memchr::memchr(b':', &bs[hi..hj]);
        let port = pk.and_then(|k| (&s[hi + k..hj]).parse::<u16>().ok());

        let l = pk.unwrap_or(hj);
        let host = (hi,l);

        let i = hj;
        let j = bs.len();

        let path = (i,j);

        let url = Url {
            serialization: s,
            scheme,
            host,
            port,
            path,
        };

        /*
        println!("Scheme: {}", url.scheme());
        println!("Host: {}", url.host_str());
        println!("Port: {:?}", url.port());
        println!("Path: {}", url.path());
        */

        Ok(url)
    }

    pub fn host_str(&self) -> &str {
        let i = self.host.0;
        let j = self.host.1;
        &self.serialization[i..j]
    }

    pub fn scheme(&self) -> &str {
        let i = self.scheme.0;
        let j = self.scheme.1;
        &self.serialization[i..j]
    }

    pub fn path(&self) -> &str {
        let i = self.path.0;
        let j = self.path.1;
        &self.serialization[i..j]
    }

    pub fn port(&self) -> u16 {
        let v = match self.scheme() {
            "http" => 80,
            "https" => 443,
            _  => 0,
        };

        self.port.unwrap_or(v)
    }
}
