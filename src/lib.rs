#![feature(conservative_impl_trait, nll)]

extern crate http;
extern crate url;
extern crate tokio;
extern crate httparse;
extern crate futures;
extern crate serde;
extern crate serde_json;
extern crate tokio_io;
extern crate bytes;
#[macro_use]
extern crate log;

use std::net::ToSocketAddrs;
use std::io;
use tokio::net::TcpStream;
use http::Request;
use futures::Future;
use http::HeaderMap;

const INITIAL_BUF_SIZE: usize = 512;

pub struct Client {}

pub struct ClientResponse {
    pub resp: http::Response<()>,
    pub body: bytes::Bytes,
}

impl ClientResponse {
    pub fn new(resp: http::Response<()>, body: bytes::Bytes) -> Self {
        ClientResponse {
            resp: resp,
            body: body,
        }
    }
}

type ClientFuture = Box<Future<Item = ClientResponse, Error = io::Error> + Send + 'static>;

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

        let query_path = uri.path_and_query()
            .expect("path and query is expected to work")
            .clone();

        extend(dst, method.as_str().as_bytes());
        extend(dst, b" ");
        extend(dst, query_path.path().as_bytes());
        let query = query_path.query();
        if query.is_some() {
            extend(dst, b"?");
            extend(dst, query.unwrap().as_bytes());
        }
        extend(dst, b" ");
        extend(dst, b"HTTP/1.1\r\nHost: ");
        extend(dst, host.as_bytes());
        extend(dst, b"\r\n");

        write_headers(&req.headers(), dst);

        extend(dst, b"\r\n");

        println!("{}", String::from_utf8(dst_vec.clone()).unwrap());

        let base_fut = tcp.and_then(move |stream| {
            tokio::io::write_all(stream, dst_vec)
                .and_then(|(stream, vec)| {
                    let initial_vec: Vec<u8> = vec![0; INITIAL_BUF_SIZE];

                    tokio_io::io::read(stream, initial_vec)
                })
                .and_then(|(stream, vec, read_len)| {
                    let mut headers = [httparse::EMPTY_HEADER; 16];
                    let mut res = httparse::Response::new(&mut headers);
                    let mut content_length = 0;

                    for header in res.headers.iter() {
                        if header.name == "Content-Length" {
                            let val = String::from_utf8_lossy(header.value);
                            content_length = val.parse().unwrap_or(0);
                            break;
                        }
                    }

                    let status = res.parse(&vec).expect("response should not be broken");
                    let code = res.code.unwrap_or(0);

                    if code == 301 {
                        // unhandled redirect
                        return futures::future::err(io::Error::from(io::ErrorKind::Other));
                    }

                    let mut body_complete = false;
                    let mut res_complete = false;
                    if status.is_complete() {
                        res_complete = true;
                        // check if body is already completed
                    }

                    if !body_complete {
                        return futures::future::Either::A(tokio::io::read_to_end(stream, vec)
                            .and_then(|(_, vec)| {
                                let f_res = match res_complete {
                                    true => {
                                        let mut headers = [httparse::EMPTY_HEADER; 16];
                                        let mut res = httparse::Response::new(&mut headers);
                                        let status =
                                            res.parse(&vec).expect("response should not be broken");

                                        res
                                    }
                                    false => res,
                                };

                                let body = bytes::BytesMut::with_capacity(vec.len()).freeze();
                                let client_res = ClientResponse::new(get_res(f_res), body);

                                futures::future::ok(client_res)
                            }));
                    }

                    let body = bytes::BytesMut::with_capacity(vec.len()).freeze();
                    let client_res = ClientResponse::new(get_res(res), body);

                    futures::future::Either::B(futures::future::ok(client_res))
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

#[inline]
fn get_res(res: httparse::Response) -> http::Response<()> {
    http::Response::builder()
        .status(res.code.unwrap_or(0))
        .body(())
        .unwrap()
}