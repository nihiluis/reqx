extern crate http;
extern crate httparse;
extern crate tokio;
extern crate reqrs;
extern crate futures;

use futures::Future;

fn main() {
    let _request = http::Request::get("https://www.rust-lang.org/")
        .body(())
        .unwrap();

    let c = reqrs::Client {};

    let jfut = c.json::<(), ()>(_request).map_err(|_| ());
    tokio::run(jfut);
}