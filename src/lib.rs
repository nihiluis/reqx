#![feature(nll)]

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
                .and_then(|(stream, _)| {
                    let initial_vec: Vec<u8> = vec![0; INITIAL_BUF_SIZE];
                    tokio_io::io::read(stream, initial_vec)
                })
                .and_then(|(stream, mut vec, _)| {
                    let mut content_length = 0;

                    let mut body_complete = false;
                    let mut res_complete = false;
                    let mut chunked_encoding = false;

                    let f_res: Option<http::Response<()>>;
                    let code: u16;

                    {
                        let mut headers = [httparse::EMPTY_HEADER; 16];
                        let mut res = httparse::Response::new(&mut headers);

                        res_complete |= res.parse(&vec)
                            .expect("response should not be broken")
                            .is_complete();

                        code = res.code.unwrap_or(0);

                        for header in res.headers.iter() {
                            if header.name == "Content-Length" {
                                let val = String::from_utf8_lossy(header.value);
                                content_length = val.parse().unwrap_or(0);
                                break;
                            } else if header.name == "Transfer-Encoding" {
                                chunked_encoding |= String::from_utf8_lossy(header.value) ==
                                    "chunked";
                            }
                        }

                        f_res = match res_complete {
                            true => Some(get_res(res)),
                            false => None,
                        };
                    }

                    trace!("No Content-Length found, Chunked {}", chunked_encoding);

                    let mut new_target_length = 0;

                    if content_length != 0 && content_length <= INITIAL_BUF_SIZE {
                        body_complete = true;
                    } else {
                        if content_length != 0 {
                            new_target_length = content_length - INITIAL_BUF_SIZE;
                        } else {
                            new_target_length = INITIAL_BUF_SIZE;
                        }
                    }

                    if code == 301 {
                        // unhandled redirect
                        return futures::future::Either::B(
                            futures::future::err(io::Error::from(io::ErrorKind::Other)),
                        );
                    }

                    if !body_complete {
                        futures::future::Either::A(tokio::io::read_exact(stream, vec![0; new_target_length]).and_then(
                            move |(_, vec2)| {
                                let f_res = match res_complete {
                                    false => {
                                        let mut headers = [httparse::EMPTY_HEADER; 16];
                                        let mut n_res = httparse::Response::new(&mut headers);

                                        n_res.parse(&vec2).expect("response should not be broken");

                                        get_res(n_res)
                                    }
                                    true => f_res.unwrap(),
                                };

                                vec.extend_from_slice(&vec2);
                                println!("{}", String::from_utf8(vec.clone()).unwrap());

                                let body = bytes::BytesMut::with_capacity(vec.len()).freeze();
                                let client_res = ClientResponse::new(f_res, body);

                                futures::future::ok(client_res)
                            },
                        ))
                    } else {
                        let body = bytes::BytesMut::with_capacity(vec.len()).freeze();
                        let client_res = ClientResponse::new(f_res.unwrap(), body);

                        futures::future::Either::B(futures::future::ok(client_res))
                    }
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