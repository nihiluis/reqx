extern crate http;
extern crate httparse;
extern crate tokio;
extern crate reqrs;
extern crate futures;

use futures::Future;

fn main() {
    let _request = http::Request::get(
        "https://min-api.cryptocompare.com/data/histoday?fsym=BTC&tsym=USD&limit=5&aggregate=3&e=CCCAGG")
        .body(())
        .unwrap();

    let c = reqrs::Client {};

    let jfut = c.json::<(), ()>(_request).map_err(|e| {
        println!("{:?}", e);

        ()
    });
    tokio::run(jfut);
}