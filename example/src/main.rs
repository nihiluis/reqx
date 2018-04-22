extern crate tokio;
extern crate reqx;
extern crate futures;
extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;

use futures::Future;

#[derive(Deserialize, Debug)]
pub struct HeartbeatDataWrapper {
    pub message: String,
    pub data: Option<HeartbeatData>,
}

#[derive(Deserialize, Debug)]
pub struct HeartbeatData {
    pub start: bool,
    pub id: i32,
}

fn main() {
    let request = reqx::ClientRequest {
        url: "http://localhost:1995/trader/data/replace?base=btc&counter=usd&exchange=gdax&id=3",
        body: None,
    };

    let c = reqx::Client {};

    let jfut = c.json_get::<HeartbeatDataWrapper>(request).map_err(|e| {
        println!("err: {:?}", e);
        ()
    });

    tokio::run(jfut.map(|x| {
        println!("{:?}", x);

        ()
    }));
}