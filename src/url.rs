
#[derive(Debug)]
pub struct Url {
    serialization: String,
    scheme: (u16, u16),
    host: (u16, u16), 
    port: Option<u16>,
    path: (u16, u16),
}

#[derive(Debug)]
pub enum Error {
    UnsupportedLength,
    Ascii,
    Scheme,
    Path,
    Host,
}

impl Url {
    pub fn parse(s: &str) -> Result<Self, Error> {
        if !s.is_ascii() {
            return Err(Error::Ascii);
        }

        let bs = s.as_bytes();
        let si = memchr::memmem::find(bs, b"://").ok_or(Error::Scheme)?;
        let scheme = (0,si as u16);
        let hi = si + 3;

        let hj = memchr::memchr(b'/', &bs[hi..]).ok_or(Error::Host)?;
        let hj = hi + hj;
        let pk = memchr::memchr(b':', &bs[hi..hj]);
        let port = pk.and_then(|k| (&s[hi + k..hj]).parse::<u16>().ok());

        let l = pk.unwrap_or(hj) as u16;
        let host = (hi as u16,l);

        let i = hj as u16;
        let j = bs.len() as u16;

        let path = (i,j);

        let url = Url {
            serialization: s.to_string(),
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
    pub fn serialization(&self) -> &str {
        self.serialization.as_str()
    }

    pub fn host_str(&self) -> &str {
        let i = self.host.0 as usize;
        let j = self.host.1 as usize;
        &self.serialization[i..j]
    }

    pub fn scheme(&self) -> &str {
        let i = self.scheme.0 as usize;
        let j = self.scheme.1 as usize;
        &self.serialization[i..j]
    }

    pub fn path(&self) -> &str {
        let i = self.path.0 as usize;
        let j = self.path.1 as usize;
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
