use hyper::{
    header::{HeaderValue, AUTHORIZATION},
    Request,
};
use parking_lot::RwLock;
use tracing::{info, trace};

use std::{
    fmt,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{self, Poll},
    time::{Duration, Instant},
};

use crate::{token, Credentials, Token, TokenSource};

pub struct AddAuthorization<S> {
    inner: Arc<RwLock<Inner>>,
    service: S,
}

impl AddAuthorization<()> {
    pub async fn init<S>(service: S) -> AddAuthorization<S> {
        AddAuthorization { inner: Arc::new(RwLock::new(Inner::init().await)), service }
    }

    pub fn init_with<S>(source: impl Into<TokenSource>, service: S) -> AddAuthorization<S> {
        AddAuthorization { inner: Arc::new(RwLock::new(Inner::init_with(source))), service }
    }
}

enum State {
    Uninitialized,
    Fetching {
        retry: u8,
        fut: Pin<Box<dyn Future<Output = token::Result<Token>> + Send + 'static>>,
    },
    Fetched(Cache),
}

struct Inner {
    state: State,
    source: TokenSource,
    max_retry: u8,
}

impl Inner {
    async fn init() -> Self {
        Self::init_with(Credentials::default().await)
    }

    fn init_with(s: impl Into<TokenSource>) -> Self {
        Self { state: State::Uninitialized, source: s.into(), max_retry: 5 }
    }

    fn can_skip_poll_ready(&self) -> bool {
        matches!(self.state, State::Fetched(ref cache) if !cache.expired(Instant::now()))
    }

    fn poll_ready(&mut self, cx: &mut task::Context<'_>) -> Poll<()> {
        loop {
            match self.state {
                State::Uninitialized => {
                    trace!("token is uninitialized");
                    self.state = State::Fetching { retry: 0, fut: Box::pin(self.source.token()) };
                    continue;
                }
                State::Fetching { ref retry, ref mut fut } => match fut.as_mut().poll(cx) {
                    Poll::Ready(r) => match r.and_then(|t| t.into_pairs()) {
                        Ok((value, expiry)) => {
                            self.state = State::Fetched(Cache::new(value, expiry));
                            trace!("token updated: expiry={:?}", expiry);
                            return Poll::Ready(());
                        }
                        Err(err) => {
                            info!("an error occurred: retry={}, err={:?}", retry, err);
                            if *retry >= self.max_retry {
                                panic!("max retry exceeded: retry={}, last error={:?}", retry, err);
                            }
                            self.state = State::Fetching {
                                retry: retry + 1,
                                fut: Box::pin(self.source.token()),
                            };
                            continue;
                        }
                    },
                    Poll::Pending => return Poll::Pending,
                },
                State::Fetched(ref cache) => {
                    if !cache.expired(Instant::now()) {
                        return Poll::Ready(());
                    }
                    trace!("token is expired: expiry={:?}", cache.expiry);
                    self.state = State::Fetching { retry: 0, fut: Box::pin(self.source.token()) };
                    continue;
                }
            }
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
        if self.inner.read().can_skip_poll_ready() {
            return self.service.poll_ready(cx);
        }
        match self.inner.write().poll_ready(cx) {
            Poll::Ready(()) => self.service.poll_ready(cx),
            Poll::Pending => Poll::Pending,
        }
    }

    fn call(&mut self, mut req: Request<B>) -> Self::Future {
        match self.inner.read().state {
            State::Fetched(ref cache) => req.headers_mut().insert(AUTHORIZATION, cache.value()),
            _ => unreachable!(),
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
