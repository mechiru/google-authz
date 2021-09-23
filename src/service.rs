use hyper::{
    header::{HeaderValue, AUTHORIZATION},
    Request,
};
use parking_lot::RwLock;
use tracing::{info, trace};

use std::{fmt, future::Future, pin::Pin, sync::Arc, task::{self, Poll}, time::{Duration, Instant}};

use crate::{Credentials, Token, TokenSource, token::Error};

pub struct AddAuthorization<S> {
    inner: Arc<Inner>,
    service: S,
}

impl AddAuthorization<()> {
    pub async fn init<S>(service: S) -> AddAuthorization<S> {
        AddAuthorization { inner: Arc::new(Inner::init().await), service }
    }

    pub fn init_with<S>(source: impl Into<TokenSource>, service: S) -> AddAuthorization<S> {
        AddAuthorization { inner: Arc::new(Inner::init_with(source)), service }
    }
}

struct Inner {
    source: Arc<TokenSource>,
    cache: RwLock<Option<Cache>>,
    future: RwLock<Option<Pin<Box<dyn Future<Output = Result<Token, Error>>>>>>,
    max_retry: u8,
}

impl Inner {
    async fn init() -> Self {
        Self::init_with(Credentials::default().await)
    }

    fn init_with(s: impl Into<TokenSource>) -> Self {
        Self {
            source: Arc::new(s.into()),
            cache: RwLock::new(None),
            max_retry: 5,
            future: RwLock::new(None),
        }
    }

    fn poll_ready(&self, cx: &mut task::Context<'_>) -> Poll<()> {
        if let Some(ref cache) = *self.cache.read() {
            if !cache.expired(Instant::now()) {
                return Poll::Ready(());
            }
            trace!("token is expired: expiry={:?}", cache.expiry);
        } else {
            trace!("token is uninitialized");
        }

        let mut lock = self.cache.write();
        trace!("start token update...");

        if let Some(ref cache) = *lock {
            if !cache.expired(Instant::now()) {
                trace!("token is already updated: expiry={:?}", cache.expiry);
                return Poll::Ready(());
            }
        }

        let mut future_write_lock = self.future.write();
        let poll_result = if let Some(fut) =  &mut *future_write_lock {
            // We are already waiting on a future; poll it.
            fut.as_mut().poll(cx)
        } else {
            // Drop the write lock so that we don't hold it across an `await`.
            drop(future_write_lock);

            // We are not yet waiting on a future, so initialize one.
            let source = self.source.clone();
            let mut fut: Pin<Box<dyn Future<Output=Result<Token, Error>>>> = Box::pin(async move {
                let r = source.token().await;
                r
            });
            
            let result = fut.as_mut().poll(cx);

            *self.future.write() = Some(fut);
            result
        };

        let cache = match poll_result {
            Poll::Ready(r)  => match r.and_then(|t| t.into_pairs()) {
                Ok((value, expiry)) => Cache::new(value, expiry),
                Err(err) => panic!("Error: {:?}", err)
            },
            Poll::Pending => return Poll::Pending
        };

        trace!("token updated: expiry={:?}", cache.expiry);
        *lock = Some(cache);
        Poll::Ready(())
    }
}

#[derive(Clone)]
struct Cache {
    value: HeaderValue,
    expiry: Instant,
}

impl Cache {
    fn new(value: HeaderValue, expiry: Instant) -> Self {
        Self { value, expiry }
    }

    fn expired(&self, at: Instant) -> bool {
        const EXPIRY_DELTA: Duration = Duration::from_secs(10);
        self.expiry.checked_duration_since(at).map(|dur| dur < EXPIRY_DELTA).unwrap_or(true)
    }

    fn value(&self) -> HeaderValue {
        self.value.clone()
    }
}

impl<S, B> tower_service::Service<Request<B>> for AddAuthorization<S>
where
    S: tower_service::Service<Request<B>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.inner.poll_ready(cx) {
            Poll::Ready(()) => self.service.poll_ready(cx),
            Poll::Pending => Poll::Pending,
        }
    }

    fn call(&mut self, mut req: Request<B>) -> Self::Future {
        match *self.inner.cache.read() {
            Some(ref cache) => req.headers_mut().insert(AUTHORIZATION, cache.value()),
            None => unreachable!(),
        };
        self.service.call(req)
    }
}

impl<S: Clone> Clone for AddAuthorization<S> {
    fn clone(&self) -> Self {
        Self { inner: self.inner.clone(), service: self.service.clone() }
    }
}

impl<S: fmt::Debug> fmt::Debug for AddAuthorization<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AddAuthorization").field("service", &self.service).finish()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn cache_expiry() {
        let now = Instant::now();
        let c = Cache::new(HeaderValue::from_static("value"), now);
        assert!(c.expired(now - Duration::from_secs(5)));
        assert!(!c.expired(now - Duration::from_secs(30)))
    }
}
