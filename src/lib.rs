//#![feature(nll)]

extern crate http;
extern crate url;
extern crate tokio;
extern crate httparse;
extern crate futures;
extern crate serde;
extern crate serde_json;
extern crate tokio_io;
extern crate bytes;
extern crate byteorder;
#[macro_use]
extern crate log;

mod pool;

use std::net::ToSocketAddrs;
use std::io;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use http::Request;
use http::Uri;
use futures::Future;
use http::HeaderMap;
use bytes::{BufMut, BytesMut};
use tokio::util::FutureExt;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use pool::{PooledStream, Pool};

const INITIAL_BUF_SIZE: usize = 512;

pub struct Client {
    pool: Arc<Mutex<Pool>>,
}

pub struct ClientRequest<'a, A> {
    pub method: http::Method,
    pub url: &'a str,
    pub body: Option<A>,
}

type ClientResponse = http::Response<Vec<u8>>;

impl Client {
    pub fn default() -> Self {
        Client { pool: Arc::new(Mutex::new(Pool::new())) }
    }

    fn request(
        &self,
        req: Request<Option<Vec<u8>>>,
    ) -> impl Future<Item = ClientResponse, Error = io::Error> + Send {
        let uri: Uri = req.uri().clone();
        let host = uri.host().unwrap().to_owned();
        let port = if let Some(p) = uri.port() { p } else { 80 };

        let (parts, body) = req.into_parts();
        let method = &parts.method;

        let content_length = match &body {
            Some(x) => x.len(),
            None => 0,
        };

        let mut headers = parts.headers;
        headers.insert(
            http::header::CONTENT_LENGTH,
            http::header::HeaderValue::from_str(content_length.to_string().as_ref())
                .expect("Header Value from Content Length may not fail"),
        );

        let mut socket_addrs = (host.as_ref(), port).to_socket_addrs().unwrap();
        let socket_addr = socket_addrs.next().unwrap();

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

        write_headers(&mut headers, dst);

        extend(dst, b"\r\n");

        if content_length > 0 {
            extend(dst, &body.unwrap());
        }

        self.begin_read((Arc::new(host), port), dst_vec)
    }

    fn begin_read(
        &self,
        key: (Arc<String>, u16),
        dst_vec: Vec<u8>,
    ) -> impl Future<Item = ClientResponse, Error = io::Error> + Send {
        let when = Instant::now() + Duration::from_secs(5);

        /*        self.pool
            .lock()
            .expect("unable to get pool lock")
            .get_conn(key)
            */
        let mut socket_addrs = ((*key.0).as_ref(), key.1).to_socket_addrs().unwrap();
        let socket_addr = socket_addrs.next().unwrap();
        TcpStream::connect(&socket_addr).and_then(move |stream| {
            tokio::io::write_all(stream, dst_vec)
                .and_then(|(stream, _)| {
                    let initial_vec: Vec<u8> = vec![0; INITIAL_BUF_SIZE];
                    tokio_io::io::read(stream, initial_vec)
                })
                .and_then(move |(stream, mut vec, _)| {
                    // TODO this is missing atm
                    let content_length: usize;

                    let mut body_complete = false;
                    let mut res_complete = false;
                    let chunked_encoding: bool;

                    let f_res: Option<http::Response<()>>;
                    let code: u16;

                    {
                        let mut headers = [httparse::EMPTY_HEADER; 16];
                        let mut res = httparse::Response::new(&mut headers);

                        res_complete |= res.parse(&vec)
                            .expect("response should not be broken")
                            .is_complete();

                        code = res.code.unwrap_or(0);
                        chunked_encoding = check_chunk_encoded(&res);
                        content_length = check_content_length(&res) as usize;

                        f_res = match res_complete {
                            true => Some(get_res(res)),
                            false => None,
                        };
                    }

                    let mut body_start_index = 0;
                    if res_complete {
                        body_start_index = get_body_start_index(&vec);
                    }

                    if content_length != 0 &&
                        content_length + body_start_index <= INITIAL_BUF_SIZE ||
                        vec.len() < INITIAL_BUF_SIZE
                    {
                        vec.truncate(body_start_index + content_length);
                        body_complete = true;
                    }

                    if code == 301 {
                        // unhandled redirect
                        error!("Unable to handle redirect.");
                        return futures::future::Either::B(
                            futures::future::err(io::Error::from(io::ErrorKind::Other)),
                        );
                    }

                    if !body_complete {
                        futures::future::Either::A(self.continue_read(
                            stream,
                            vec,
                            res_complete,
                            f_res,
                            chunked_encoding,
                            body_start_index,
                            content_length,
                        ))
                    } else {
                        join_body(&mut vec, body_start_index, content_length);

                        let client_res = apply_body_to_res(f_res.unwrap(), vec);

                        futures::future::Either::B(futures::future::ok(client_res))
                    }
                })
                .deadline(when)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
        })
    }

    fn continue_read(
        &self,
        stream: TcpStream,
        vec: Vec<u8>,
        res_complete: bool,
        f_res: Option<http::Response<()>>,
        chunk_encoded: bool,
        body_start_index: usize,
        content_length: usize,
    ) -> impl Future<Item = ClientResponse, Error = io::Error> + Send {
        // maybe I should resolve body_start_index here later
    /*
    if chunk_encoded && body_start_index != 0 {

    } else {
    */
        // this is not preferred because it's slow
        self.read_all(
            stream,
            vec,
            res_complete,
            chunk_encoded,
            f_res,
            body_start_index,
            content_length,
        )
    }

    fn read_all(
        &self,
        stream: TcpStream,
        vec: Vec<u8>,
        res_complete: bool,
        chunk_encoding: bool,
        f_res: Option<http::Response<()>>,
        mut body_start_index: usize,
        content_length: usize,
    ) -> impl Future<Item = ClientResponse, Error = io::Error> + Send {
        // this can be easily replaced with read_to_end
        let fut = futures::future::loop_fn((stream, vec, 0, 0), |(stream,
          mut vec,
          mut read_len,
          zero_len_count)| {
            tokio_io::io::read(stream, vec![0; 1024]).and_then(move |(stream, mut vec2, len)| {
                if len == 0 {
                    return Ok(futures::future::Loop::Break(
                        (stream, vec, read_len, zero_len_count),
                    ));
                }

                vec2.truncate(len);
                read_len += len;

                vec.extend_from_slice(&vec2);

                Ok(futures::future::Loop::Continue(
                    (stream, vec, read_len, zero_len_count),
                ))
            })
        });

        fut.and_then(move |(stream, mut vec, _, _)| {
            let f_res = match res_complete {
                false => {
                    body_start_index = get_body_start_index(&vec);
                    let lines_to_body = count_lines(&vec[0..body_start_index]) - 3;

                    let headers_size: usize;
                    if lines_to_body > 0 {
                        headers_size = lines_to_body as usize + 2;
                    } else {
                        headers_size = 32;
                    }

                    let mut headers = vec![httparse::EMPTY_HEADER; headers_size];
                    let mut n_res = httparse::Response::new(&mut headers);

                    n_res.parse(&vec).expect("response should not be broken");

                    get_res(n_res)
                }
                true => f_res.unwrap(),
            };

            match chunk_encoding {
                true => join_chunks(&mut vec, body_start_index),
                // not sure if the last param here is correct
                false => join_body(&mut vec, body_start_index, content_length),
            };

            let client_res = apply_body_to_res(f_res, vec);

            futures::future::ok(client_res)
        })
    }

    pub fn string<'a>(
        &self,
        client_req: ClientRequest<'a, Vec<u8>>,
    ) -> impl Future<Item = String, Error = io::Error> + Send {
        let req = http::Request::builder()
            .uri(client_req.url)
            .method(client_req.method)
            //.header(http::header::CONNECTION, "close")
            .body(client_req.body)
            .unwrap();

        self.request(req).and_then(|r| {
            let (_, body) = r.into_parts();
            let body_string = String::from_utf8(body);

            futures::future::result(body_string).map_err(|e| {
                io::Error::new(io::ErrorKind::Other, e)
            })
        })
    }

    pub fn json<'a, A, B>(
        &self,
        client_req: ClientRequest<'a, A>,
    ) -> impl Future<Item = B, Error = io::Error>
    where
        B: serde::de::DeserializeOwned,
        A: serde::Serialize,
    {

        let mut req_builder = http::Request::builder();
        req_builder
            .uri(client_req.url)
            .method(client_req.method)
            .header(http::header::CONTENT_TYPE, "application/json")
            //.header(http::header::CONNECTION, "close")
            .header(http::header::ACCEPT, "application/json");

        let req = match client_req.body {
            Some(b) => {
                let parsed =
                    serde_json::to_string(&b).map_err(|e| io::Error::new(io::ErrorKind::Other, e));

                if parsed.is_err() {
                    return futures::future::Either::A(futures::future::err(parsed.err().unwrap()));
                }

                let vec = parsed.unwrap().into_bytes();
                req_builder.body(Some(vec))
            }
            None => req_builder.body(None),
        }.unwrap();

        futures::future::Either::B(self.request(req).and_then(|r| {
            let (_, body) = r.into_parts();
            //let body_str = std::str::from_utf8(&body);
            let parsed: Result<B, serde_json::Error> = serde_json::from_slice(&body);

            futures::future::result(parsed).map_err(|e| {
                error!("Unable to parse json GET response: {:?}", e);

                io::Error::new(io::ErrorKind::Other, e)
            })
        }))
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

#[inline]
fn _get_res_with_body(res: httparse::Response, body: Vec<u8>) -> http::Response<Vec<u8>> {
    http::Response::builder()
        .status(res.code.unwrap_or(0))
        .body(body)
        .unwrap()
}

#[inline]
fn apply_body_to_res(res: http::Response<()>, body: Vec<u8>) -> http::Response<Vec<u8>> {
    let (parts, _) = res.into_parts();

    http::Response::from_parts(parts, body)
}

#[inline]
fn count_lines(slice: &[u8]) -> i16 {
    let mut n = 0;

    for b in slice.iter() {
        if *b == b'\n' {
            n += 1;
        }
    }

    n
}

#[inline]
fn get_body_start_index(slice: &[u8]) -> usize {
    let mut previous_r = false;
    let mut body_start_index = 0;
    for (i, n) in slice.iter().enumerate() {
        if previous_r && *n == b'\n' {
            if &slice[i - 3..i + 1] == b"\r\n\r\n" {
                body_start_index = i + 1;
                break;
            }
        }
        previous_r = *n == b'\r';
    }

    body_start_index
}

fn check_chunk_encoded(res: &httparse::Response) -> bool {
    for header in res.headers.iter() {
        if header.name == "Transfer-Encoding" {
            let val = String::from_utf8_lossy(header.value);
            if val == "chunked" {
                return true;
            }
        }
    }

    false
}

fn check_content_length(res: &httparse::Response) -> u32 {
    for header in res.headers.iter() {
        if header.name == "Content-Length" {
            let buf_str = std::str::from_utf8(&header.value).unwrap();
            return buf_str.parse::<u32>().unwrap();
        }
    }

    0
}

fn join_body(vec: &mut Vec<u8>, body_start_index: usize, body_length: usize) {
    let body_end: usize;
    if body_length == 0 {
        body_end = vec.len() - 2;
    } else {
        body_end = body_start_index + body_length;
    }

    // -2 to remove trailing chars
    vec.drain(..body_start_index);
    vec.truncate(body_end - body_start_index);
}

fn join_chunks(vec: &mut Vec<u8>, body_start_index: usize) {
    let mut start_index = body_start_index;

    loop {
        let chunk_size = get_chunk_size(vec, start_index);
        if chunk_size == 0 {
            break;
        }
        let chunk_end = start_index + chunk_size as usize;
        // I dont know if this is correct
        vec.drain((chunk_end + 1)..(chunk_end + 3));
        //vec2.extend_from_slice(&vec[start_index..chunk_end + 1]);

        start_index = chunk_end + 2;
    }
}

// returns (chunk_size, byte size of the chunk_size)
fn get_chunk_size(vec: &mut Vec<u8>, start_index: usize) -> u16 {
    let buf_len = 16;
    let mut buf = BytesMut::with_capacity(buf_len);

    if vec[start_index] == b'\0' {
        return 0;
    }

    loop {
        let val = vec[start_index];
        vec.remove(start_index);
        if val == b'\r' {
            continue;
        }
        if val == b'\n' {
            break;
        }
        if val == b'\0' {
            return 0;
        }

        if buf.len() == buf_len {
            break;
        }

        buf.put(val);
    }

    let buf = buf.freeze();

    let buf_str = std::str::from_utf8(&buf).unwrap();
    if buf.len() == 0 {
        return 0;
    }
    let chunk_size = u16::from_str_radix(&buf_str, 16).unwrap();

    chunk_size
}