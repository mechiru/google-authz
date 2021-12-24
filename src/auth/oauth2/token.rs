use std::{
    convert::TryFrom,
    fmt,
    time::{Duration, Instant},
};

use futures_util::future::BoxFuture;
use hyper::header::HeaderValue;

use crate::auth;

#[derive(Clone)]
pub(crate) struct Token {
    pub value: HeaderValue,
    pub expiry: Instant,
}

impl Token {
    pub fn new(value: HeaderValue, expiry: Instant) -> Self {
        Self { value, expiry }
    }

    pub fn expired(&self, at: Instant) -> bool {
        const EXPIRY_DELTA: Duration = Duration::from_secs(10);
        self.expiry.checked_duration_since(at).map(|dur| dur < EXPIRY_DELTA).unwrap_or(true)
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct Response {
    pub token_type: String,
    pub access_token: String,
    pub expires_in: u64,
}

impl TryFrom<Response> for Token {
    type Error = auth::Error;

    fn try_from(response: Response) -> Result<Self, Self::Error> {
        if !response.token_type.is_empty()
            && !response.access_token.is_empty()
            && response.expires_in > 0
        {
            let value = format!("{} {}", response.token_type, response.access_token);
            if let Ok(value) = HeaderValue::from_str(&value) {
                let expiry = Instant::now() + Duration::from_secs(response.expires_in);
                return Ok(Token::new(value, expiry));
            }
        }
        Err(auth::Error::TokenFormat(response))
    }
}

pub(crate) type ResponseFuture = BoxFuture<'static, auth::Result<Response>>;

pub(crate) trait Fetcher: fmt::Debug + 'static {
    fn fetch(&self) -> ResponseFuture;
}
