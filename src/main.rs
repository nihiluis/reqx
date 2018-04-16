extern crate http;
extern crate httparse;
extern crate tokio;
extern crate reqrs;
extern crate futures;

use futures::Future;

fn main() {
    let request = http::Request::get(
        "https://jsonplaceholder.typicode.com/users")
        .header(http::header::USER_AGENT, "Mozilla/5.0 (X11; Linux x86_64; rv:59.0) Gecko/20100101 Firefox/59.0")
        .header(http::header::CONNECTION, "close")
        //.header(http::header::ACCEPT_ENCODING, "gzip, deflate, br")
        .body(())
        .unwrap();

    let c = reqrs::Client {};

    let jfut = c.json::<(), ()>(request).map_err(|e| {
        println!("{:?}", e);

        ()
    });
    tokio::run(jfut.map(|x| {
        println!("{:?}", x.resp);

        ()
    }));
}