use hyper::{
    header::{HeaderValue, AUTHORIZATION},
    Request,
};
use parking_lot::RwLock;
use tracing::{info, trace};

use std::{fmt, future::Future, pin::Pin, sync::{Arc, Mutex}, task::{self, Poll}, time::{Duration, Instant}};

use crate::{token::Error, Credentials, Token, TokenSource};

pub struct AddAuthorization<S> {
    inner: Arc<Inner>,
    service: S,
}

impl AddAuthorization<()> {
    pub async fn init<S>(service: S) -> AddAuthorization<S> {
        AddAuthorization {
            inner: Arc::new(Inner::init().await),
            service,
        }
    }

    pub fn init_with<S>(source: impl Into<TokenSource>, service: S) -> AddAuthorization<S> {
        AddAuthorization {
            inner: Arc::new(Inner::init_with(source)),
            service,
        }
    }
}

/// Represents the state of the current token.
enum TokenStatus {
    /// Represents the state where we have already received and cached a token
    /// from the server, which may be expired.
    Cached(Cache),

    /// Represents the state where we are in the process of obtaining a token
    /// from the server, possibly after some unsuccessful attempts.
    Waiting {
        future: Mutex<Pin<Box<dyn Future<Output = Result<Token, Error>> + Send>>>,
        retry: u8,
    },
}

struct Inner {
    source: Arc<TokenSource>,
    status: RwLock<TokenStatus>,
    max_retry: u8,
}

impl Inner {
    async fn init() -> Self {
        Self::init_with(Credentials::default().await)
    }

    /// Build a boxed future for fetching a token from the given TokenSource reference.
    fn do_fetch(source: Arc<TokenSource>) -> Pin<Box<dyn Future<Output = Result<Token, Error>> + Send>> {
        Box::pin(async move {
            let r = source.token().await;
            r
        })
    }

    fn init_with(s: impl Into<TokenSource>) -> Self {
        let source = Arc::new(s.into());
        let future = Mutex::new(Inner::do_fetch(source.clone()));

        Self {
            source,
            max_retry: 5,
            status: RwLock::new(TokenStatus::Waiting { retry: 0, future }),
        }
    }

    fn cache(&self) -> Option<Cache> {
        match &*self.status.read() {
            TokenStatus::Cached(cache) => Some(cache.clone()),
            _ => None
        }
    }

    fn poll_ready(&self, cx: &mut task::Context<'_>) -> Poll<()> {
        if let Some((expiry, expired)) = self.cache().map(|c| (c.expiry, c.expired(Instant::now()))) {
            if expired {
                let mut status = self.status.write();
                let mut future = Inner::do_fetch(self.source.clone());
                let _ = future.as_mut().poll(cx);
                *status = TokenStatus::Waiting { retry: 0, future: Mutex::new(future) };
    
                trace!("token is expired: expiry={:?}", expiry);
                return Poll::Pending;    
            } else {
                return Poll::Ready(());
            }
        }

        let mut status = self.status.write();
        if let TokenStatus::Waiting { future, retry } = &mut *status {
            let mut future = if let Ok(future) = future.try_lock() {
                future
            } else {
                return Poll::Pending;
            };
            let result = future.as_mut().poll(cx);
            drop(future);

            match result {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(r) => match r.and_then(|t| t.into_pairs()) {
                    Ok((value, expiry)) => {
                        trace!("token updated: expiry={:?}", expiry);
                        *status = TokenStatus::Cached(Cache::new(value, expiry));
                        return Poll::Ready(())
                    },
                    Err(err) => {
                        info!("an error occurred: retry={}, err={:?}", retry, err);
                        if *retry > self.max_retry {
                            panic!("max retry exceeded: retry={}, last error={:?}", retry, err);
                        }
                        let mut future = Inner::do_fetch(self.source.clone());
                        let _ = future.as_mut().poll(cx);
                        *status = TokenStatus::Waiting { retry: *retry+1, future: Mutex::new(future) };
                        return Poll::Pending;
                    }
                },
            }
        } else {
            // It would take a rare race condition to get here, but if we do, we
            // poll so that we re-enter from the top to process the new state.
            return Poll::Pending;
        }
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
        self.expiry
            .checked_duration_since(at)
            .map(|dur| dur < EXPIRY_DELTA)
            .unwrap_or(true)
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
        match self.inner.cache() {
            Some(ref cache) => req.headers_mut().insert(AUTHORIZATION, cache.value()),
            None => unreachable!(),
        };
        self.service.call(req)
    }
}

impl<S: Clone> Clone for AddAuthorization<S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            service: self.service.clone(),
        }
    }
}

impl<S: fmt::Debug> fmt::Debug for AddAuthorization<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AddAuthorization")
            .field("service", &self.service)
            .finish()
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
