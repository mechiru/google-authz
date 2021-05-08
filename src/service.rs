use hyper::{
    header::{HeaderValue, AUTHORIZATION},
    Request,
};
use parking_lot::RwLock;
use tracing::{info, trace};

use std::{
    fmt,
    future::Future,
    sync::Arc,
    task::{self, Poll},
    time::{Duration, Instant},
};

use crate::{Credentials, TokenSource};

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
    source: TokenSource,
    cache: RwLock<Option<Cache>>,
    max_retry: u8,
}

impl Inner {
    async fn init() -> Self {
        Self::init_with(Credentials::default().await)
    }

    fn init_with(s: impl Into<TokenSource>) -> Self {
        Self { source: s.into(), cache: RwLock::new(None), max_retry: 5 }
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

        let fut = self.source.token();
        pin_utils::pin_mut!(fut);

        let mut retry = 0;
        let cache = loop {
            match fut.as_mut().poll(cx) {
                Poll::Ready(r) => match r.and_then(|t| t.into_pairs()) {
                    Ok((value, expiry)) => break Cache::new(value, expiry),
                    Err(err) => {
                        info!("an error occurred: retry={}, err={:?}", retry, err);
                        if retry >= self.max_retry {
                            panic!("max retry exceeded: retry={}, last error={:?}", retry, err);
                        }
                        retry += 1;
                    }
                },
                Poll::Pending => {}
            }
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
            Poll::Pending => unreachable!(),
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
