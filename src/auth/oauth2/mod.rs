use std::{
    convert::TryInto,
    fmt,
    sync::Arc,
    task::{self, Poll},
    time::Instant,
};

use hyper::{
    header::{self, AUTHORIZATION},
    Request,
};
use parking_lot::RwLock;
use tracing::trace;

use crate::{auth, sync::RefGuard};

mod http;
pub(super) mod token;

mod metadata;
mod service_account;
mod user;

pub use metadata::Metadata;
pub use service_account::ServiceAccount;
pub use user::User;

#[derive(Clone)]
pub(super) struct Oauth2 {
    inner: Arc<RwLock<Inner>>,
}

impl Oauth2 {
    pub fn new<T>(fetcher: T) -> Self
    where
        T: token::Fetcher,
    {
        Self {
            inner: Arc::new(RwLock::new(Inner {
                state: State::NotFetched,
                fetcher: Box::new(fetcher),
            })),
        }
    }

    pub fn poll_ready(&mut self, cx: &mut task::Context<'_>) -> Poll<auth::Result<()>> {
        if self.inner.read().can_skip_poll_ready() {
            return Poll::Ready(Ok(()));
        }
        self.inner.write().poll_ready(cx)
    }

    #[inline]
    pub fn add_header<B>(&self, mut req: Request<B>) -> Request<B> {
        req.headers_mut().insert(AUTHORIZATION, self.inner.read().value());
        req
    }
}

impl fmt::Debug for Oauth2 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Oauth2").field("inner", &self.inner).finish()
    }
}

struct Inner {
    state: State,
    fetcher: Box<dyn token::Fetcher>,
}

impl Inner {
    #[inline]
    fn can_skip_poll_ready(&self) -> bool {
        matches!(self.state, State::Fetched { ref current } if !current.expired(Instant::now()))
    }

    #[inline]
    fn poll_ready(&mut self, cx: &mut task::Context<'_>) -> Poll<auth::Result<()>> {
        macro_rules! poll_future {
            ($future:expr) => {
                match $future.get_mut().as_mut().poll(cx) {
                    Poll::Ready(resp) => match resp.and_then(TryInto::<token::Token>::try_into) {
                        Ok(token) => {
                            trace!("fetched token: expiry={:?}", token.expiry);
                            self.state = State::Fetched { current: token };
                            return Poll::Ready(Ok(()));
                        }
                        Err(err) => return Poll::Ready(Err(err)),
                    },
                    Poll::Pending => return Poll::Pending,
                }
            };
        }

        loop {
            match self.state {
                State::NotFetched => {
                    trace!("token is not fetched");
                    self.state = State::Fetching { future: RefGuard::new(self.fetcher.fetch()) };
                    continue;
                }
                State::Fetching { ref mut future } => poll_future!(future),
                State::Refetching { ref mut future, .. } => poll_future!(future),
                State::Fetched { ref current } => {
                    if !current.expired(Instant::now()) {
                        return Poll::Ready(Ok(()));
                    }
                    trace!("token will expire: expiry={:?}", current.expiry);
                    self.state = State::Refetching {
                        future: RefGuard::new(self.fetcher.fetch()),
                        last: current.clone(),
                    };
                    continue;
                }
            }
        }
    }

    #[inline]
    fn value(&self) -> header::HeaderValue {
        match self.state {
            State::Fetched { ref current } => current.value.clone(),
            State::Refetching { ref last, .. } => last.value.clone(),
            _ => unreachable!("invalid state: {:?}", self.state),
        }
    }
}

impl fmt::Debug for Inner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Inner").field("state", &self.state).field("fetcher", &self.fetcher).finish()
    }
}

enum State {
    NotFetched,
    Fetching { future: RefGuard<token::ResponseFuture> },
    Refetching { future: RefGuard<token::ResponseFuture>, last: token::Token },
    Fetched { current: token::Token },
}

impl fmt::Debug for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFetched => write!(f, "NotFetched"),
            Self::Fetching { .. } => write!(f, "Fetching"),
            Self::Refetching { .. } => write!(f, "Refetching"),
            Self::Fetched { .. } => write!(f, "Fetched"),
        }
    }
}
