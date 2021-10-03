use std::{
    fmt,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{self, Poll},
    time::{Duration, Instant},
};

use hyper::{
    header::{HeaderValue, AUTHORIZATION},
    Request,
};
use parking_lot::RwLock;
use tracing::{info, trace};

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
        fut: RefGuard<Pin<Box<dyn Future<Output = token::Result<Token>> + Send + 'static>>>,
    },
    Fetched,
}

/// RefGuard wraps a `Send` type to make it `Sync`, by ensuring that it is only
/// ever accessed through a &mut pointer.
struct RefGuard<T: Send>(T);

impl<T: Send> RefGuard<T> {
    pub fn new(value: T) -> Self {
        RefGuard(value)
    }

    pub fn get_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

unsafe impl<T: Send> Sync for RefGuard<T> {}

struct Inner {
    state: State,
    cache: Option<Cache>,
    source: TokenSource,
    max_retry: u8,
}

impl Inner {
    async fn init() -> Self {
        Self::init_with(Credentials::default().await)
    }

    fn init_with(s: impl Into<TokenSource>) -> Self {
        Self { state: State::Uninitialized, cache: None, source: s.into(), max_retry: 5 }
    }

    #[inline]
    fn cache_ref(&self) -> &Cache {
        self.cache.as_ref().unwrap()
    }

    fn can_skip_poll_ready(&self) -> bool {
        matches!(self.state, State::Fetched) && !self.cache_ref().expired(Instant::now())
    }

    fn poll_ready(&mut self, cx: &mut task::Context<'_>) -> Poll<()> {
        loop {
            match self.state {
                State::Uninitialized => {
                    trace!("token is uninitialized");
                    self.state = State::Fetching { retry: 0, fut: RefGuard::new(Box::pin(self.source.token())) };
                    continue;
                }
                State::Fetching { ref retry, ref mut fut } => match fut.get_mut().as_mut().poll(cx) {
                    Poll::Ready(r) => match r.and_then(|t| t.into_pairs()) {
                        Ok((value, expiry)) => {
                            self.cache = Some(Cache::new(value, expiry));
                            self.state = State::Fetched;
                            trace!("token updated: expiry={:?}", expiry);
                            return Poll::Ready(());
                        }
                        Err(err) => {
                            if *retry < self.max_retry {
                                info!("an error occurred: retry={}, err={:?}", retry, err);
                            } else {
                                panic!("max retry exceeded: retry={}, last error={:?}", retry, err);
                            }
                            self.state = State::Fetching {
                                retry: retry + 1,
                                fut: RefGuard::new(Box::pin(self.source.token())),
                            };
                            continue;
                        }
                    },
                    Poll::Pending => return Poll::Pending,
                },
                State::Fetched => {
                    let cache = self.cache_ref();
                    if !cache.expired(Instant::now()) {
                        return Poll::Ready(());
                    }
                    trace!("token will expire: expiry={:?}", cache.expiry);
                    self.state = State::Fetching { retry: 0, fut: RefGuard::new(Box::pin(self.source.token())) };
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
        req.headers_mut().insert(AUTHORIZATION, self.inner.read().cache_ref().value());
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
    fn compile_test() {
        #[derive(Clone)]
        struct Counter {
            cur: i32,
        }

        impl Counter {
            fn new() -> Self {
                Counter { cur: 0 }
            }
        }

        impl<B> tower_service::Service<Request<B>> for Counter {
            type Response = i32;
            type Error = i32;
            type Future = Pin<Box<dyn Future<Output = Result<i32, i32>> + Send + 'static>>;

            fn poll_ready(&mut self, _: &mut task::Context<'_>) -> Poll<Result<(), Self::Error>> {
                Poll::Ready(Ok(()))
            }

            fn call(&mut self, _: Request<B>) -> Self::Future {
                self.cur += 1;
                let current = self.cur;
                Box::pin(async move { Ok(current) })
            }
        }

        fn assert_send<T: Send>(_: T) {}
        fn assert_sync<T: Sync>(_: T) {}

        let svc = AddAuthorization::init_with(
            Credentials::from_json(
                br#"{
  "client_id": "xxx.apps.googleusercontent.com",
  "client_secret": "secret-xxx",
  "refresh_token": "refresh-xxx",
  "type": "authorized_user"
}"#,
                &[],
            ),
            Counter::new(),
        );
        assert_send(svc.clone());
        assert_sync(svc);
    }

    #[test]
    fn cache_expiry() {
        let now = Instant::now();
        let c = Cache::new(HeaderValue::from_static("value"), now);
        assert!(c.expired(now - Duration::from_secs(5)));
        assert!(!c.expired(now - Duration::from_secs(30)))
    }
}
