use std::{
    fmt,
    future::{self, Ready},
    task::{self, Poll},
};

use futures_util::{
    future::{Either, MapErr},
    TryFutureExt as _,
};
use hyper::Request;

use crate::{
    auth::{self, Auth, Config},
    credentials::Credentials,
};

/// Represents an inner service error or Google authentication error.
#[derive(thiserror::Error, Debug)]
pub enum Error<E> {
    #[error("inner service error: {0}")]
    Service(E),
    #[error("google authentication error: {0}")]
    GoogleAuthz(auth::Error),
}

pub struct Builder<S> {
    config: Config,
    credentials: Option<Credentials>,
    service: S,
}

impl Builder<()> {
    pub fn new<S>(service: S) -> Builder<S> {
        Builder { config: Default::default(), credentials: Default::default(), service }
    }
}

impl<S> Builder<S> {
    #[cfg(not(feature = "tonic"))]
    pub fn enforce_https(mut self, enforce_https: bool) -> Self {
        self.config.enforce_https = enforce_https;
        self
    }

    pub fn max_retry(mut self, max_retry: u8) -> Self {
        self.config.max_retry = max_retry;
        self
    }

    pub fn credentials(mut self, credentials: impl Into<Option<Credentials>>) -> Self {
        self.credentials = credentials.into();
        self
    }

    pub async fn build<B>(self) -> GoogleAuthz<S>
    where
        S: tower_service::Service<Request<B>>,
    {
        let Builder { config, credentials, service } = self;
        let credentials = match credentials {
            Some(credentials) => credentials,
            None => Credentials::new().await,
        };
        GoogleAuthz { auth: Auth::new(credentials, config), service }
    }
}

pub struct GoogleAuthz<S> {
    auth: Auth,
    service: S,
}

impl GoogleAuthz<()> {
    pub async fn new<S, B>(service: S) -> GoogleAuthz<S>
    where
        S: tower_service::Service<Request<B>>,
    {
        Self::builder(service).build().await
    }

    pub fn builder<S>(service: S) -> Builder<S> {
        Builder::new(service)
    }
}

impl<S: Clone> Clone for GoogleAuthz<S> {
    fn clone(&self) -> Self {
        Self { auth: self.auth.clone(), service: self.service.clone() }
    }
}

impl<S: fmt::Debug> fmt::Debug for GoogleAuthz<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GoogleAuthz")
            .field("auth", &self.auth)
            .field("service", &self.service)
            .finish()
    }
}

impl<S, B> tower_service::Service<Request<B>> for GoogleAuthz<S>
where
    S: tower_service::Service<Request<B>>,
{
    type Response = S::Response;
    type Error = Error<S::Error>;
    #[allow(clippy::type_complexity)]
    type Future = Either<
        MapErr<S::Future, fn(S::Error) -> Self::Error>,
        Ready<Result<Self::Response, Self::Error>>,
    >;

    fn poll_ready(&mut self, cx: &mut task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.auth.poll_ready(cx) {
            Poll::Ready(Ok(())) => self.service.poll_ready(cx).map_err(Error::Service),
            Poll::Ready(Err(err)) => Poll::Ready(Err(Error::GoogleAuthz(err))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        match self.auth.call(req) {
            Ok(req) => Either::Left(self.service.call(req).map_err(Error::Service)),
            Err(err) => Either::Right(future::ready(Err(Error::GoogleAuthz(err)))),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_compile() {
        fn assert_send<T: Send>(_: &T) {}
        fn assert_sync<T: Sync>(_: &T) {}

        #[derive(Clone)]
        struct Counter(i32);

        impl tower_service::Service<Request<hyper::Body>> for Counter {
            type Response = i32;
            type Error = i32;
            type Future = futures_util::future::BoxFuture<'static, Result<i32, i32>>;

            fn poll_ready(&mut self, _: &mut task::Context<'_>) -> Poll<Result<(), Self::Error>> {
                Poll::Ready(Ok(()))
            }

            fn call(&mut self, _: Request<hyper::Body>) -> Self::Future {
                self.0 += 1;
                let current = self.0;
                Box::pin(async move { Ok(current) })
            }
        }

        let svc = GoogleAuthz::new(Counter(0)).await;
        assert_send(&svc);
        assert_sync(&svc);
    }
}
