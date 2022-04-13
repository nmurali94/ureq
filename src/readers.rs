use crate::stream::Stream;
use std::io::{self, Read};

type CarryOver = arrayvec::ArrayVec<u8, 2048>;

pub(crate) struct ComboReader {
    pub co: CarryOver,
    pub st: Stream,
}

impl Read for ComboReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let c = self.co.as_slice().read(buf)?;
        if c == 0 {
            self.st.read(buf)
        } else {
            let _ = self.co.drain(..c);
            Ok(c)
        }
    }
}
//
// ErrorReader returns an error for every read.
// The error is as close to a clone of the underlying
// io::Error as we can get.
pub(crate) struct ErrorReader(io::Error);

impl Read for ErrorReader {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(self.0.kind(), self.0.to_string()))
    }
}

pub(crate) struct ReadIterator<'a, R, const N: usize> { 
    pub r: &'a mut R,
}

impl <'a, R, const N: usize> Iterator for ReadIterator<'a, R, N>
where R: Read
{
    type Item = std::io::Result<([u8; N], usize)>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut buf = [0u8; N];
        match self.r.read(&mut buf) {
            Ok(0) => None,
            Ok(i) => Some(Ok((buf, i))),
            Err(e) => Some(Err(e)),
        }
    }
}
