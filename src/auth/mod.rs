use std::{
    convert::TryFrom,
    task::{self, Poll},
};

use hyper::Request;

use crate::Credentials;

mod api_key;
mod error;
mod oauth2;

pub use error::*;
use oauth2::{token::Fetcher, Metadata, Oauth2, ServiceAccount, User};

#[derive(Clone, Debug)]
pub(crate) struct Config {
    #[cfg(not(feature = "tonic"))]
    pub enforce_https: bool,
    pub max_retry: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            #[cfg(not(feature = "tonic"))]
            enforce_https: true,
            max_retry: 3,
        }
    }
}

#[derive(Clone, Debug)]
enum Inner {
    None,
    ApiKey(api_key::ApiKey),
    Oauth2(oauth2::Oauth2),
}

impl TryFrom<(Credentials, &Config)> for Inner {
    type Error = AuthBuilderError;
    fn try_from(
        (credentials, config): (Credentials, &Config),
    ) -> std::result::Result<Self, AuthBuilderError> {
        let fetcher: Box<dyn Fetcher> = match credentials {
            Credentials::None => return Ok(Self::None),
            Credentials::ApiKey(key) => return Ok(Self::ApiKey(api_key::ApiKey::new(key))),
            Credentials::User(user) => Box::new(User::new(user)),
            Credentials::ServiceAccount(sa) => Box::new(ServiceAccount::try_new(sa)?),
            Credentials::Metadata(meta) => Box::new(Metadata::try_new(meta)?),
        };
        Ok(Self::Oauth2(Oauth2::new(fetcher, config.max_retry)))
    }
}

// https://cloud.google.com/docs/authentication
#[derive(Clone, Debug)]
pub(crate) struct Auth {
    inner: Inner,
    #[cfg(not(feature = "tonic"))]
    enforce_https: bool,
}

impl Auth {
    pub fn try_new(
        credentials: Credentials,
        config: Config,
    ) -> std::result::Result<Self, AuthBuilderError> {
        let inner = Inner::try_from((credentials, &config))?;
        Ok(Self {
            inner,
            #[cfg(not(feature = "tonic"))]
            enforce_https: config.enforce_https,
        })
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
        #[cfg(not(feature = "tonic"))]
        if self.enforce_https {
            check_https(req.uri().scheme_str())?;
        }

        match self.inner {
            Inner::None => Ok(req),
            Inner::ApiKey(ref key) => Ok(key.add_query(req)?),
            Inner::Oauth2(ref oauth2) => Ok(oauth2.add_header(req)),
        }
    }
}

#[inline]
#[cfg(not(feature = "tonic"))]
fn check_https(scheme: Option<&'_ str>) -> Result<()> {
    match scheme {
        Some("https") => Ok(()),
        _ => Err(Error::EnforceHttps(scheme.map(ToOwned::to_owned))),
    }
}
