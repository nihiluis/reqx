extern crate http;
extern crate httparse;
extern crate tokio;
extern crate reqx;
extern crate futures;

use futures::Future;

fn main() {
    let request = http::Request::get("http://api.jikan.moe/anime/1/episodes")
        .header(http::header::ACCEPT, "application/json")
        .header(http::header::CONNECTION, "close")
        .body(())
        .unwrap();

    let c = reqx::Client {};

    let jfut = c.json::<(), ()>(request).map_err(|_| {

        ()
    });

    tokio::run(jfut.map(|x| {
        println!("{:?}", x);

        ()
    }));
}