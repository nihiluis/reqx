#![feature(conservative_impl_trait, nll)]

extern crate http;
extern crate url;
extern crate tokio;
extern crate httparse;
extern crate futures;
extern crate serde;
extern crate serde_json;
extern crate tokio_io;
#[macro_use]
extern crate log;

use std::net::ToSocketAddrs;
use std::io;
use tokio::net::TcpStream;
use tokio_io::{AsyncRead, AsyncWrite};
use http::Request;
use futures::{Future, Async};
use http::HeaderMap;

pub struct Client {}

type ClientFuture = Box<Future<Item = (), Error = io::Error> + Send + 'static>;

impl Client {
    fn request<A, B>(self, req: Request<A>) -> ClientFuture {
        let uri = req.uri().clone();
        let host = uri.host().unwrap();
        let port = if let Some(p) = uri.port() { p } else { 80 };
        let method = req.method();

        let mut socket_addrs = (host, port).to_socket_addrs().unwrap();
        let socket_addr = socket_addrs.next().unwrap();

        let tcp = TcpStream::connect(&socket_addr);

        let mut dst_vec: Vec<u8> = Vec::new();
        let dst = &mut dst_vec;

        extend(dst, method.as_str().as_bytes());
        extend(dst, b" ");
        extend(dst, uri.path().as_bytes());
        extend(dst, b" ");
        extend(dst, b"HTTP/1.1\r\nHost: ");
        extend(dst, host.as_bytes());
        extend(dst, b"\r\n");

        write_headers(&req.headers(), dst);

        extend(dst, b"\r\n");

        let base_fut = tcp.and_then(move |mut s: TcpStream| {
            let poll_fut = match s.poll_write(&mut dst_vec) {
                Ok(Async::Ready(_)) => {
                    trace!("poll_write Ready");
                    futures::future::ok(())
                }
                Ok(Async::NotReady) => {
                    // do I need to retry now?
                    trace!("poll_write NotReady");
                    futures::future::err(io::Error::from(io::ErrorKind::Other))
                }
                Err(_) => {
                    error!("poll_write error");
                    futures::future::err(io::Error::from(io::ErrorKind::Other))
                }
            };

            poll_fut.and_then(|_| {
                s.poll_read
            })
        });

        Box::new(base_fut)
    }

    pub fn json<A, B>(self, req: Request<A>) -> ClientFuture {
        self.request::<A, B>(req)
    }
}

#[inline]
fn extend(dst: &mut Vec<u8>, data: &[u8]) {
    dst.extend_from_slice(data);
}

fn write_headers(headers: &HeaderMap, dst: &mut Vec<u8>) {
    for (name, value) in headers {
        extend(dst, name.as_str().as_bytes());
        extend(dst, b": ");
        extend(dst, value.as_bytes());
        extend(dst, b"\r\n");
    }
}