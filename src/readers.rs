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

// ErrorReader returns an error for every read.
// The error is as close to a clone of the underlying
// io::Error as we can get.
pub(crate) struct ErrorReader(io::Error);

impl Read for ErrorReader {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(self.0.kind(), self.0.to_string()))
    }
}

/**
 * Iterators to emulate control loops for Read
 */

pub struct ReadIterator<'a, R> { 
    r: &'a mut R,
    d: &'a mut [u8],
}

impl <'a, R> ReadIterator<'a, R> 
where R: Read 
{
    pub fn new(r: &'a mut R, d: &'a mut [u8]) -> Self {
        ReadIterator { r, d }
    }
}

impl <'a, R> Iterator for ReadIterator<'a, R>
where R: Read
{
    type Item = std::io::Result<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        let v = self.r.read(self.d);
        match v {
            Ok(0) => None,
            _ => Some(v),
        }
    }
}

pub struct ReadToEndIterator<'a, R> { 
    r: &'a mut R,
    d: &'a mut [u8],
    l: usize,
}

impl <'a, R> ReadToEndIterator<'a, R> 
where R: Read 
{
    pub fn new(r: &'a mut R, d: &'a mut [u8]) -> Self {
        ReadToEndIterator { r, d, l: 0 }
    }
}

impl <'a, R> Iterator for ReadToEndIterator<'a, R>
where R: Read
{
    type Item = std::io::Result<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        let v = self.r.read(&mut self.d[self.l..]);
        match v {
            Ok(0) => None,
            Ok(n) => {self.l += n; Some(Ok(n)) },
            Err(e) => Some(Err(e)),
        }
    }
}

pub struct ConsumingReadIterator<'a, R, F> { 
    r: &'a mut R,
    d: &'a mut [u8],
    l: usize,
    f: &'a mut F,
}

impl <'a, R, F> ConsumingReadIterator<'a, R, F> 
where R: Read, F: FnMut(&mut [u8]) -> usize 
{
    pub fn new(r: &'a mut R, d: &'a mut [u8], f: &'a mut F) -> Self {
        ConsumingReadIterator { r, d, l: 0, f}
    }
}

impl <'a, R, F> Iterator for ConsumingReadIterator<'a, R, F>
where R: Read, F: FnMut(&mut [u8]) -> usize
{
    type Item = std::io::Result<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        let v = self.r.read(&mut self.d[self.l..]);
        match v {
            Ok(0) => {
                if self.l > 0 {
                    let c = (self.f) (&mut self.d[..self.l]);
                    self.l = 0;
                    Some(Ok(c))
               } else { None }
            },
            Ok(n) => {
                let t = self.l + n;
                let consume = (self.f) (&mut self.d[..t]);
                self.d.copy_within(consume..t, 0);
                self.l = t - consume;
                Some(Ok(consume))
            },
            Err(e) => Some(Err(e)),
        }
    }
}
