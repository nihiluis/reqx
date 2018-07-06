extern crate tokio;
extern crate reqx;
extern crate futures;
extern crate serde;
extern crate http;
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
        url: "http://localhost:1995/trader/data/replace?base=eth&counter=usdt&exchange=binance&id=108",
        body: None,
        method: http::method::Method::GET,
    };

    let c = reqx::Client::default();

    let jfut = c.json::<(), ReplacementDataWrapper<USDNumeric>>(request).map_err(|e| {
        println!("err: {:?}", e);

        ()
    });

    tokio::run(jfut.map(|x| {
        println!("{:?}", x);

        ()
    }));
}

#[derive(Deserialize, Debug)]
pub struct IncrementDataWrapper<T> {
    pub data: Option<IncrementData<T>>,
    pub message: String,
}

#[derive(Deserialize, Debug)]
pub struct IncrementData<T> {
    #[serde(rename = "lastPrice")]
    pub last_price: T,
    pub last: LastInfo<T>,
    #[serde(rename = "orderBook")]
    pub order_book: OrderBook<T>,
    #[serde(rename = "type")]
    pub data_type: DataType,
    pub base: String,
    pub counter: String,
    pub exchange: String,
}

#[derive(Deserialize, Debug)]
pub struct ReplacementDataWrapper<T> {
    pub data: Option<ReplacementData<T>>,
    pub message: String,
}

#[derive(Deserialize, Debug)]
pub struct ReplacementData<T> {
    pub time: u64,
    #[serde(rename = "lastPrice")]
    pub last_price: T,
    pub minute: Quotes<T>,
    pub hour: Quotes<T>,
    pub last: LastInfo<T>,
    #[serde(rename = "orderBook")]
    pub order_book: OrderBook<T>,
    #[serde(rename = "type")]
    pub data_type: DataType,
    pub base: String,
    pub counter: String,
    pub exchange: String,
}

#[derive(Clone, Copy, Debug)]
pub enum Currency {
    BTC,
    USD,
}

#[derive(Clone, Copy, Debug)]
pub enum Exchange {
    GDAX,
}

#[derive(Clone, Copy, Debug)]
pub enum TradeMode {
    Simple,
    Cautious,
    Aggro,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum DataType {
    #[serde(rename = "increment")]
    INCREMENT,
    #[serde(rename = "replace")]
    REPLACE,
}

impl Currency {
    pub fn from_str<'a>(s: &'a str) -> Option<Self> {
        match s {
            "btc" => Some(Currency::BTC),
            "usd" => Some(Currency::USD),
            _ => None,
        }
    }

    pub fn to_str(&self) -> &'static str {
        match *self {
            Currency::BTC => "btc",
            Currency::USD => "usd",
        }
    }
}

impl Exchange {
    pub fn from_str<'a>(s: &'a str) -> Option<Self> {
        match s {
            "gdax" => Some(Exchange::GDAX),
            _ => None,
        }
    }

    pub fn to_str(&self) -> &'static str {
        match *self {
            Exchange::GDAX => "gdax",
        }
    }
}


impl DataType {
    pub fn to_str(&self) -> &'static str {
        match *self {
            DataType::INCREMENT => "increment",
            DataType::REPLACE => "replace",
        }
    }
}

impl TradeMode {
    pub fn from_str<'a>(s: &'a str) -> Option<Self> {
        match s {
            "simple" => Some(TradeMode::Simple),
            "cautious" => Some(TradeMode::Cautious),
            "aggro" => Some(TradeMode::Aggro),
            _ => None,
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct OrderBook<T> {
    #[serde(rename = "amountAsk")]
    pub amount_ask: i32,
    #[serde(rename = "amountBid")]
    pub amount_bid: i32,
    pub asks: Vec<Order<T>>,
    pub bids: Vec<Order<T>>,
}

#[derive(Serialize, Deserialize, Copy, Clone, Debug)]
pub struct Order<T> {
    pub price: T,
    #[serde(rename = "remainingAmount")]
    pub remaining_amount: T,
}

impl<T: NumericOps<T>> Order<T> {
    pub fn empty() -> Self {
        Order {
            price: T::empty(),
            remaining_amount: T::empty(),
        }
    }
}

impl<T: NumericOps<T>> OrderBook<T> {
    pub fn empty() -> Self {
        OrderBook {
            amount_ask: 0,
            amount_bid: 0,
            asks: Vec::new(),
            bids: Vec::new(),
        }
    }

    pub fn is_initialized(&self) -> bool {
        self.amount_ask > 0 && self.amount_bid > 0 && self.asks.len() > 0
    }
}

#[derive(Deserialize, Copy, Clone, Debug)]
pub struct LastInfo<T> {
    pub amount: i32,
    pub volume: T,
    #[serde(rename = "volume5m")]
    pub volume_5m: T,
    #[serde(rename = "amountAskBidRatio")]
    pub amount_ask_bid_ratio: f64,
    #[serde(rename = "volumeAskBidRatio")]
    pub volume_ask_bid_ratio: f64,
    pub close: T,
    pub high: T,
    pub low: T,
    pub open: T,
    pub quality: T,
}

impl<T: NumericOps<T>> LastInfo<T> {
    pub fn new() -> Self {
        LastInfo {
            amount: 0,
            volume: T::empty(),
            volume_5m: T::empty(),
            amount_ask_bid_ratio: 0.0,
            volume_ask_bid_ratio: 0.0,
            close: T::empty(),
            high: T::empty(),
            low: T::empty(),
            open: T::empty(),
            quality: T::empty(),
        }
    }
}

#[derive(Clone, Deserialize, Debug)]
pub struct Quotes<T> {
    pub time: Vec<u64>,
    pub close: Vec<T>,
    pub open: Vec<T>,
    pub high: Vec<T>,
    pub low: Vec<T>,
    pub volume: Vec<T>,
}

use std::ops::{Add, Div, Mul, Sub, AddAssign, SubAssign, Rem};
use std::cmp::{PartialEq, PartialOrd, Ordering};
use std::fmt;
use std::f64;
use serde::de::{self, Visitor, Deserializer, Deserialize, DeserializeOwned};
use serde::{Serializer, Serialize};

#[derive(Copy, Clone, Debug)]
pub struct USDNumeric {
    pub n: f64,
}

impl fmt::Display for USDNumeric {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "${:.2}", self.n / 100.0)
    }
}

pub trait NumericOps<T>
    : Add<Output = T>
    + AddAssign
    + SubAssign
    + Sub<Output = T>
    + Mul<Output = T>
    + Div<Output = T>
    + Add<f64, Output = T>
    + AddAssign<f64>
    + SubAssign<f64>
    + Sub<f64, Output = T>
    + Mul<f64, Output = T>
    + Div<f64, Output = T>
    + Rem<usize, Output = T>
    + Rem<f64, Output = T>
    + Numeric<T>
    + PartialEq<f64>
    + PartialOrd<f64>
    + PartialEq
    + PartialOrd
    + Copy
    + fmt::Display
    + DeserializeOwned
    + Serialize {
}

impl<T> NumericOps<T> for T
where
    T: Add<Output = T>
        + AddAssign
        + SubAssign
        + Sub<Output = T>
        + Mul<Output = T>
        + Div<Output = T>
        + Add<f64, Output = T>
        + AddAssign<f64>
        + SubAssign<f64>
        + Sub<f64, Output = T>
        + Mul<f64, Output = T>
        + Div<f64, Output = T>
        + Rem<usize, Output = T>
        + Rem<f64, Output = T>
        + Numeric<T>
        + PartialEq<f64>
        + PartialOrd<f64>
        + PartialEq
        + PartialOrd
        + Copy
        + fmt::Display
        + DeserializeOwned
        + Serialize,
{
}

impl Serialize for USDNumeric {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_f64(self.n / 100.0)
    }
}

struct USDNumericVisitor;

impl<'de> Visitor<'de> for USDNumericVisitor {
    type Value = USDNumeric;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("something")
    }

    fn visit_f64<E>(self, value: f64) -> Result<USDNumeric, E>
    where
        E: de::Error,
    {
        Ok(USDNumeric { n: value * 100.0 })
    }

    fn visit_i64<E>(self, value: i64) -> Result<USDNumeric, E>
    where
        E: de::Error,
    {
        Ok(USDNumeric { n: value as f64 * 100.0 })
    }

    fn visit_u64<E>(self, value: u64) -> Result<USDNumeric, E>
    where
        E: de::Error,
    {
        Ok(USDNumeric { n: value as f64 * 100.0 })
    }
}

impl<'de> Deserialize<'de> for USDNumeric {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_f64(USDNumericVisitor)
    }
}

pub const USD_ZERO: USDNumeric = USDNumeric { n: 0.0_f64 };

pub trait Numeric<T> {
    fn is_zero(&self) -> bool;
    fn empty() -> T;
    fn sqrt(&self) -> T;
    fn is_nan(&self) -> bool;
    fn new(i32) -> T;
    fn get_digits(&self) -> u32;
    fn abs(&self) -> T;
}

impl Numeric<USDNumeric> for USDNumeric {
    #[inline]
    fn is_zero(&self) -> bool {
        self.n == 0.0_f64
    }

    #[inline]
    fn empty() -> USDNumeric {
        USD_ZERO
    }

    #[inline]
    fn sqrt(&self) -> USDNumeric {
        USDNumeric { n: self.n.sqrt() }
    }

    #[inline]
    fn is_nan(&self) -> bool {
        self.n == f64::NAN
    }

    #[inline]
    fn new(i: i32) -> USDNumeric {
        USDNumeric { n: i as f64 }
    }

    #[inline]
    fn get_digits(&self) -> u32 {
        let mut x = *self;

        let mut digits = 0;
        if x < 0.0 {
            digits = 1;
        }

        while x > 1.0 {
            x = x / 10.0;
            digits += 1;
        }

        digits
    }

    #[inline]
    fn abs(&self) -> Self {
        if *self < 0.0 {
            return *self * -1.0;
        }

        *self
    }
}

impl Add<f64> for USDNumeric {
    type Output = USDNumeric;

    #[inline]
    fn add(self, other: f64) -> USDNumeric {
        USDNumeric { n: self.n + other }
    }
}

impl Add for USDNumeric {
    type Output = USDNumeric;

    #[inline]
    fn add(self, other: USDNumeric) -> USDNumeric {
        USDNumeric { n: self.n + other.n }
    }
}

impl AddAssign<f64> for USDNumeric {
    #[inline]
    fn add_assign(&mut self, other: f64) {
        *self = USDNumeric { n: self.n + other };
    }
}

impl AddAssign for USDNumeric {
    #[inline]
    fn add_assign(&mut self, other: USDNumeric) {
        *self = USDNumeric { n: self.n + other.n };
    }
}

impl SubAssign<f64> for USDNumeric {
    #[inline]
    fn sub_assign(&mut self, other: f64) {
        *self = USDNumeric { n: self.n - other };
    }
}

impl SubAssign for USDNumeric {
    #[inline]
    fn sub_assign(&mut self, other: USDNumeric) {
        *self = USDNumeric { n: self.n - other.n };
    }
}

impl Sub<f64> for USDNumeric {
    type Output = USDNumeric;

    #[inline]
    fn sub(self, other: f64) -> USDNumeric {
        USDNumeric { n: self.n - other }
    }
}

impl Sub for USDNumeric {
    type Output = USDNumeric;

    #[inline]
    fn sub(self, other: USDNumeric) -> USDNumeric {
        USDNumeric { n: self.n - other.n }
    }
}

impl Div<f64> for USDNumeric {
    type Output = USDNumeric;

    #[inline]
    fn div(self, other: f64) -> USDNumeric {
        USDNumeric { n: self.n / other }
    }
}

impl Div for USDNumeric {
    type Output = USDNumeric;

    #[inline]
    fn div(self, other: USDNumeric) -> USDNumeric {
        USDNumeric { n: self.n / other.n }
    }
}

impl Mul<f64> for USDNumeric {
    type Output = USDNumeric;

    #[inline]
    fn mul(self, other: f64) -> USDNumeric {
        USDNumeric { n: self.n * other }
    }
}

impl Mul for USDNumeric {
    type Output = USDNumeric;

    #[inline]
    fn mul(self, other: USDNumeric) -> USDNumeric {
        USDNumeric { n: self.n * other.n }
    }
}

impl Rem<usize> for USDNumeric {
    type Output = USDNumeric;

    fn rem(self, modulus: usize) -> Self {
        USDNumeric { n: self.n % modulus as f64 }
    }
}

impl Rem<f64> for USDNumeric {
    type Output = USDNumeric;

    fn rem(self, modulus: f64) -> Self {
        USDNumeric { n: self.n % modulus }
    }
}

impl PartialEq<f64> for USDNumeric {
    fn eq(&self, other: &f64) -> bool {
        self.n == *other
    }
}

impl PartialEq for USDNumeric {
    fn eq(&self, other: &USDNumeric) -> bool {
        self.n == other.n
    }
}

impl PartialOrd<f64> for USDNumeric {
    fn partial_cmp(&self, other: &f64) -> Option<Ordering> {
        self.n.partial_cmp(&other)
    }
}

impl PartialOrd for USDNumeric {
    fn partial_cmp(&self, other: &USDNumeric) -> Option<Ordering> {
        self.n.partial_cmp(&other.n)
    }
}
