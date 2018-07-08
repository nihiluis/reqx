use std::sync::{Arc, Mutex};
use std::rc::Rc;
use std::collections::HashMap;
use tokio::net::TcpStream;
use futures::{self, Future};
use std::net::ToSocketAddrs;
use std::fmt::Arguments;
use std::io;
use tokio;
use tokio::prelude::{Read, Write, Async, AsyncRead, AsyncWrite};
use bytes;

type Key = (Arc<String>, u16);

#[derive(Debug)]
pub struct PooledStream {
    inner: Option<PooledStreamInner>,
    pool: Arc<Mutex<PoolInner>>,
}

impl PooledStream {
    pub fn new(key: Key, stream: TcpStream, pool: Arc<Mutex<PoolInner>>) -> Self {
        PooledStream {
            inner: Some(PooledStreamInner {
                key: key,
                stream: stream,
                is_closed: false,
            }),
            pool: pool,
        }
    }

    fn is_closed(&mut self) -> bool {
        self.inner.is_some() && self.inner.as_mut().unwrap().is_closed
    }
}

static EXPECT_INNER: &'static str = "inner is expected";

impl Write for PooledStream {
    fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        self.inner.as_mut().expect(EXPECT_INNER).stream.write(buf)
    }

    fn flush(&mut self) -> Result<(), io::Error> {
        self.inner.as_mut().expect(EXPECT_INNER).stream.flush()
    }

    fn write_all(&mut self, buf: &[u8]) -> Result<(), io::Error> {
        self.inner.as_mut().expect(EXPECT_INNER).stream.write_all(
            buf,
        )
    }

    fn write_fmt(&mut self, fmt: Arguments) -> Result<(), io::Error> {
        self.inner.as_mut().expect(EXPECT_INNER).stream.write_fmt(
            fmt,
        )
    }

    fn by_ref(&mut self) -> &mut Self {
        self
    }
}

impl Read for PooledStream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
        match self.inner.as_mut().unwrap().stream.read(buf) {
            Ok(0) => {
                // if the wrapped stream returns EOF (Ok(0)), that means the
                // server has closed the stream. we must be sure this stream
                // is dropped and not put back into the pool.
                println!("Reading and getting EOF, setting is_closed to true");
                self.inner.as_mut().unwrap().is_closed = true;
                Ok(0)
            }
            r => r,
        }
    }
}

impl AsyncRead for PooledStream {}

impl AsyncWrite for PooledStream {
    fn shutdown(&mut self) -> Result<Async<()>, io::Error> {
        Ok(().into())
    }

    fn write_buf<B>(&mut self, buf: &mut B) -> Result<Async<usize>, io::Error>
    where
        B: bytes::Buf,
    {
        self.inner.as_mut().expect(EXPECT_INNER).stream.write_buf(
            buf,
        )
    }
}

#[derive(Debug)]
struct PooledStreamInner {
    key: Key,
    stream: TcpStream,
    is_closed: bool,
}

pub struct Pool {
    inner: Arc<Mutex<PoolInner>>,
}

impl Clone for Pool {
    fn clone(&self) -> Pool {
        Pool {
            inner: self.inner.clone(),
        }
    }
}

impl Pool {
    pub fn new() -> Self {
        Pool { inner: Arc::new(Mutex::new(PoolInner { conns: HashMap::new() })) }
    }

    pub fn get_conn(
        &mut self,
        key: Key,
    ) -> Box<Future<Item = PooledStream, Error = io::Error> + Send + 'static> {
        let pool_clone = self.inner.clone();

        if let Ok(ref mut inner) = self.inner.try_lock() {
            println!("clearing inner");
            //inner.clear_expired_key(&key);
            println!("map {:?}", inner.conns);

            println!("got mut contains_key {} {}", key.0.as_ref(), inner.conns.contains_key(&key));
            let mut opt_streams = inner.conns.get_mut(&key);
            if let Some(ref mut streams) = opt_streams {
                println!("popping");
                let opt_stream = streams.pop();
                if let Some(stream) = opt_stream {
                    println!("reusing pool conn");
                    return Box::new(futures::future::lazy(|| futures::future::ok(stream)));
                }
            }
        }

        println!("creating pool conn");
        self.new_stream(key, pool_clone)
    }

    fn new_stream(
        &mut self,
        key: Key,
        pool_clone: Arc<Mutex<PoolInner>>,
    ) -> Box<Future<Item = PooledStream, Error = io::Error> + Send + 'static> {
        let mut socket_addrs = ((*key.0).as_ref(), key.1).to_socket_addrs().unwrap();
        let socket_addr = socket_addrs.next().unwrap();

        Box::new(TcpStream::connect(&socket_addr).and_then(move |stream| {
            let pooled_stream = PooledStream::new(key, stream, pool_clone);
            futures::future::ok(pooled_stream)
        }))
    }
}

#[derive(Debug)]
pub struct PoolInner {
    conns: HashMap<Key, Vec<PooledStream>>,
}

impl PoolInner {
    fn clear_expired_key(&mut self, key: &Key) {
        if let Some(ref mut values) = self.conns.get_mut(key) {
            values.retain(|entry| {
                if let Some(ref self_inner) = entry.inner {
                    if self_inner.is_closed {
                        println!("found closed stream");
                        return false;
                    }
                }

                true
            });
        }
    }

    fn clear_expired(&mut self) {
        self.conns.retain(|_, values| {
            values.retain(|entry| {
                if let Some(ref self_inner) = entry.inner {
                    if self_inner.is_closed {
                        return false;
                    }
                }

                true
            });

            !values.is_empty()
        })
    }

    fn push(&mut self, key: Key, pooled: PooledStreamInner, pool_ref: Arc<Mutex<PoolInner>>) {
        if self.conns.contains_key(&key) {
            println!("adding stream");
            let mut stream_vec = self.conns.get_mut(&key).unwrap();
            stream_vec.push(PooledStream {
                inner: Some(pooled),
                pool: pool_ref.clone(),
            });
        } else {
            println!("inserting stream with key {}", key.0.as_ref());
            println!("self.conns before {:?}", self.conns);
            self.conns.insert(
                key,
                vec![
                    PooledStream {
                        inner: Some(pooled),
                        pool: pool_ref.clone(),
                    },
                ],
            );
            println!("self.conns after {:?}", self.conns);
        }
    }
}

impl Drop for PooledStream {
    fn drop(&mut self) {
        println!("Dropping 1");
        if let Some(ref mut self_inner) = self.inner {
            println!("Dropping 2");
            println!("is_closed {}", self_inner.is_closed);
            if !self_inner.is_closed {
                println!("Dropping 3");
                if let Ok(ref mut pool) = self.pool.try_lock() {
                    println!("Dropping 4");
                    let inner = self.inner.take().unwrap();
                    pool.push(inner.key.clone(), inner, self.pool.clone());
                }
            }
        }
    }
}