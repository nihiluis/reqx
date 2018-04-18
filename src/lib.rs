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
extern crate byteorder;
#[macro_use]
extern crate log;

use std::net::ToSocketAddrs;
use std::io;
use tokio::net::TcpStream;
use http::Request;
use futures::Future;
use http::HeaderMap;
use byteorder::{BigEndian, ReadBytesExt};
use bytes::{BufMut, BytesMut};

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

        let base_fut = tcp.and_then(move |stream| {
            tokio::io::write_all(stream, dst_vec)
                .and_then(|(stream, _)| {
                    let initial_vec: Vec<u8> = vec![0; INITIAL_BUF_SIZE];
                    tokio_io::io::read(stream, initial_vec)
                })
                .and_then(|(stream, vec, _)| {
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
                        chunked_encoding = check_chunk_encoded(&res);

                        f_res = match res_complete {
                            true => Some(get_res(res)),
                            false => None,
                        };
                    }

                    let mut new_target_length = 0;

                    // i think the vec.len check is redundant
                    if content_length != 0 && content_length <= INITIAL_BUF_SIZE ||
                        vec.len() < INITIAL_BUF_SIZE
                    {
                        body_complete = true;
                    } else if content_length != 0 {
                        new_target_length = content_length - INITIAL_BUF_SIZE;
                    }

                    let mut body_start_index = 0;
                    let mut chunk_size = 0;
                    if res_complete {
                        body_start_index = get_body_start_index(&vec);
                    }

                    if code == 301 {
                        // unhandled redirect
                        println!("Unable to handle redirect.");
                        return futures::future::Either::B(
                            futures::future::err(io::Error::from(io::ErrorKind::Other)),
                        );
                    }

                    if !body_complete {
                        futures::future::Either::A(continue_read(
                            stream,
                            vec,
                            new_target_length,
                            res_complete,
                            f_res,
                            chunked_encoding,
                            body_start_index,
                        ))
                    } else {
                        let body = bytes::BytesMut::with_capacity(vec.len()).freeze();
                        println!("{}", String::from_utf8(vec.clone()).unwrap());
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

fn continue_read(
    stream: TcpStream,
    vec: Vec<u8>,
    new_target_length: usize,
    res_complete: bool,
    f_res: Option<http::Response<()>>,
    chunk_encoded: bool,
    body_start_index: usize,
) -> impl Future<Item = ClientResponse, Error = io::Error> {
    // maybe I should resolve body_start_index here later
    /*
    if chunk_encoded && body_start_index != 0 {

    } else {
    */
    // this is not preferred because it's slow
    read_all(
        stream,
        vec,
        res_complete,
        f_res,
        chunk_encoded,
        body_start_index,
    )
}

fn read_all(
    stream: TcpStream,
    mut vec: Vec<u8>,
    res_complete: bool,
    f_res: Option<http::Response<()>>,
    chunk_encoded: bool,
    mut body_start_index: usize,
) -> Box<Future<Item = ClientResponse, Error = io::Error> + Send + 'static> {
    // this can be easily replaced with read_to_end
    let fut = futures::future::loop_fn((stream, vec), |(stream, mut vec)| {
        tokio_io::io::read(stream, vec![0; 1024]).and_then(|(stream, vec2, read_len)| {
            if read_len == 0 {
                return Ok(futures::future::Loop::Break((stream, vec)));
            }

            vec.extend_from_slice(&vec2);

            Ok(futures::future::Loop::Continue((stream, vec)))
        })
    });

    Box::new(fut.and_then(move |(_, mut vec)| {
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

        println!("{}", String::from_utf8_lossy(&vec));
        join_chunks(&mut vec, body_start_index);

        let body = bytes::BytesMut::with_capacity(vec.len()).freeze();
        let client_res = ClientResponse::new(f_res, body);

        futures::future::ok(client_res)
    }))
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
            println!(
                "{}",
                String::from_utf8_lossy(&slice[i - 3..i])
                    .replace("\n", "_n")
                    .replace("\r", "_r")
            );
            if &slice[i - 3..i] == b"\r\n\r" {
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

fn join_chunks(vec: &mut Vec<u8>, body_start_index: usize) {
    let mut start_index = body_start_index;
    //println!("{}", String::from_utf8_lossy(vec));
    let mut vec2: Vec<u8> = Vec::new();
    loop {
        let (chunk_size, chunk_size_byte_size) = get_chunk_size(vec, start_index);

        let chunk_start = start_index; // why dont i add htis + chunk_size_byte_size + 3;
        println!("chunk_size: {}", chunk_size);
        vec2.extend_from_slice(&vec[chunk_start..chunk_start + chunk_size as usize]);

        start_index = chunk_start + chunk_size as usize + 1;

        println!("==\n\n");
    }

    let st = String::from_utf8(vec2).unwrap();
    println!("{}|{}", st.len(), st);
}

fn _remove_chunk_info_from_vec(vec: &mut Vec<u8>, body_start_index: usize) {
    let mut start_index = body_start_index;
    loop {
        let (chunk_size, chunk_size_byte_size) = get_chunk_size(vec, start_index);

        if chunk_size == 0 {
            //let mut vec_len = vec.len();
            // remove all terminating chars
            /*loop {
                println!("{}", vec[vec_len - 1]);
                if vec[vec_len - 1] != b'\r' && vec[vec_len - 1] != b'\n' {
                    println!("breaking");
                    break;
                }

                vec_len -= 1;
                vec.pop();
            }*/
            println!("truncating");
            vec.truncate(start_index);
            break;
        }

        let chunk_end = chunk_size;
        println!("start_index: {}", start_index);
        println!("chunk_size: {}", chunk_size);
        println!("chunk_end {}", chunk_end as usize);
        // if that is efficient ....

        println!("vec length {}", vec.len());
        println!("Vec:");
        //println!("{}", String::from_utf8_lossy(&vec));

        start_index = chunk_end as usize;
    }
}

// returns (chunk_size, byte size of the chunk_size)
fn get_chunk_size(vec: &mut Vec<u8>, start_index: usize) -> (u16, usize) {
    let buf_len = 16;
    let mut buf = BytesMut::with_capacity(buf_len);
    let mut chunk_size_index = start_index;


    /*println!("passed start_index {}", start_index); // is the initial_start_index wrong?
    println!(
        "start chunk range \n{}",
        String::from_utf8_lossy(&vec[start_index - 6..start_index + 4])
            .replace("\r\n", "==_rn==\r\n")
    );
    println!(
        "possible chunk range \n{}",
        String::from_utf8_lossy(&vec[start_index - 293..start_index + 500])
            .replace("\r\n", "==_rn==\r\n")
    );*/

    if vec[chunk_size_index] == b'\0' {
        println!("returning chunk size 0");
        return (0, 1);
    }

    loop {
        let val = vec[chunk_size_index];
        vec.remove(chunk_size_index);
        if val == b'\r' {
            continue;
        }
        if val == b'\n' {
            break;
        }
        if val == b'\0' {
            return (0, 0);
        }

        if buf.len() == buf_len {
            break;
        }

        buf.put(val);
    }

    let buf = buf.freeze();

    /*let chunk_size = std::io::Cursor::new(&buf)
        .read_u16::<byteorder::BigEndian>()
        .expect("KDASD");
    let mut rdr = std::io::Cursor::new(b"160d");
    let chunk_size = rdr.read_u16::<byteorder::BE>().expect(
        "Chunk size can not be read as u16",
    );*/

    let buf_str = std::str::from_utf8(&buf).unwrap();
    println!("tryong to convert {}:{}", buf_str, buf.len());
    if buf.len() == 0 {
        return (0, 0);
    }
    let chunk_size = u16::from_str_radix(&buf_str, 16).unwrap();

    (chunk_size, buf.len())
}