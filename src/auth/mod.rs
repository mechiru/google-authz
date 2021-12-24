use std::task::{self, Poll};

use hyper::Request;

use crate::Credentials;

mod api_key;
mod error;
mod oauth2;

pub use error::*;
use oauth2::{Metadata, Oauth2, ServiceAccount, User};

#[derive(Clone, Debug)]
pub(crate) struct Config {
    pub with_tonic: bool,
    pub enforce_https: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self { with_tonic: false, enforce_https: true }
    }
}

#[derive(Clone, Debug)]
enum Inner {
    None,
    ApiKey(api_key::ApiKey),
    Oauth2(oauth2::Oauth2),
}

impl From<Credentials> for Inner {
    fn from(credentials: Credentials) -> Self {
        match credentials {
            Credentials::None => Self::None,
            Credentials::ApiKey(key) => Self::ApiKey(api_key::ApiKey::new(key)),
            Credentials::User(user) => Self::Oauth2(Oauth2::new(User::new(user))),
            Credentials::ServiceAccount(sa) => Self::Oauth2(Oauth2::new(ServiceAccount::new(sa))),
            Credentials::Metadata(meta) => Self::Oauth2(Oauth2::new(Metadata::new(meta))),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Auth {
    inner: Inner,
    config: Config,
}

impl Auth {
    pub fn new(credentials: Credentials, config: Config) -> Self {
        Self { inner: credentials.into(), config }
    }

    #[inline]
    pub fn poll_ready(&mut self, cx: &mut task::Context<'_>) -> Poll<Result<()>> {
        match self.inner {
            Inner::Oauth2(ref mut oauth2) => oauth2.poll_ready(cx),
            _ => Poll::Ready(Ok(())),
        }
    }

    #[inline]
    pub fn call<B>(&self, req: Request<B>) -> Result<Request<B>> {
        if !self.config.with_tonic && self.config.enforce_https {
            check_https(req.uri().scheme_str())?;
        }

        match self.inner {
            Inner::None => Ok(req),
            Inner::ApiKey(ref key) => Ok(key.add_query(req)),
            Inner::Oauth2(ref oauth2) => Ok(oauth2.add_header(req)),
        }
    }
}

#[inline]
fn check_https(scheme: Option<&'_ str>) -> Result<()> {
    match scheme {
        Some("https") => Ok(()),
        _ => Err(Error::EnforceHttps(scheme.map(ToOwned::to_owned))),
    }
}
