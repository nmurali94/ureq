use std::io::{self, Read};

use crate::stream::Stream;
///
/// Read wrapper that returns the stream to the pool once the
/// read is exhausted (reached a 0).
///
/// *Internal API*
pub(crate) struct PoolReturnRead<R: Read + Sized + Into<Stream>> {
    // unit that contains the agent where we want to return the reader.
    // wrapped reader around the same stream
    reader: Option<R>,
}

impl<R: Read + Sized + Into<Stream>> PoolReturnRead<R> {
    pub fn new(reader: R) -> Self {
        PoolReturnRead {
            reader: Some(reader),
        }
    }

    fn return_connection(&mut self) -> io::Result<()> {
        // guard we only do this once.
        if let Some(reader) =  self.reader.take() {
            // bring back stream here to either go into pool or dealloc
            let mut stream = reader.into();
            if !stream.is_poolable() {
                // just let it deallocate
                return Ok(());
            }

            // ensure stream can be reused
            stream.reset()?;

        }

        Ok(())
    }

    fn do_read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.reader.as_mut() {
            None => Ok(0),
            Some(reader) => reader.read(buf),
        }
    }
}

impl<R: Read + Sized + Into<Stream>> Read for PoolReturnRead<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let amount = self.do_read(buf)?;
        // only if the underlying reader is exhausted can we send a new
        // request to the same socket. hence, we only return it now.
        if amount == 0 {
            self.return_connection()?;
        }
        Ok(amount)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poolkey_new() {
        // Test that PoolKey::new() does not panic on unrecognized schemes.
        PoolKey::new(&Url::parse("zzz:///example.com").unwrap(), None);
    }

    #[test]
    fn pool_connections_limit() {
        // Test inserting connections with different keys into the pool,
        // filling and draining it. The pool should evict earlier connections
        // when the connection limit is reached.
        let pool = ConnectionPool::new_with_limits(10, 1);
        let hostnames = (0..pool.max_idle_connections * 2).map(|i| format!("{}.example", i));
        let poolkeys = hostnames.map(|hostname| PoolKey {
            scheme: "https".to_string(),
            hostname,
            port: Some(999),
            proxy: None,
        });
        for key in poolkeys.clone() {
            pool.add(key, Stream::from_vec(vec![]))
        }
        assert_eq!(pool.len(), pool.max_idle_connections);

        for key in poolkeys.skip(pool.max_idle_connections) {
            let result = pool.remove(&key);
            assert!(result.is_some(), "expected key was not in pool");
        }
        assert_eq!(pool.len(), 0)
    }

    #[test]
    fn pool_per_host_connections_limit() {
        // Test inserting connections with the same key into the pool,
        // filling and draining it. The pool should evict earlier connections
        // when the per-host connection limit is reached.
        let pool = ConnectionPool::new_with_limits(10, 2);
        let poolkey = PoolKey {
            scheme: "https".to_string(),
            hostname: "example.com".to_string(),
            port: Some(999),
            proxy: None,
        };

        for _ in 0..pool.max_idle_connections_per_host * 2 {
            pool.add(poolkey.clone(), Stream::from_vec(vec![]))
        }
        assert_eq!(pool.len(), pool.max_idle_connections_per_host);

        for _ in 0..pool.max_idle_connections_per_host {
            let result = pool.remove(&poolkey);
            assert!(result.is_some(), "expected key was not in pool");
        }
        assert_eq!(pool.len(), 0);
    }

    #[test]
    fn pool_checks_proxy() {
        // Test inserting different poolkeys with same address but different proxies.
        // Each insertion should result in an additional entry in the pool.
        let pool = ConnectionPool::new_with_limits(10, 1);
        let url = Url::parse("zzz:///example.com").unwrap();

        pool.add(PoolKey::new(&url, None), Stream::from_vec(vec![]));
        assert_eq!(pool.len(), 1);

        pool.add(
            PoolKey::new(&url, Some(Proxy::new("localhost:9999").unwrap())),
            Stream::from_vec(vec![]),
        );
        assert_eq!(pool.len(), 2);

        pool.add(
            PoolKey::new(
                &url,
                Some(Proxy::new("user:password@localhost:9999").unwrap()),
            ),
            Stream::from_vec(vec![]),
        );
        assert_eq!(pool.len(), 3);
    }
}
