use std::{error, fmt};

use hyper::{body::HttpBody, client::connect::Connect, Body, Request, Response};
use tower_service::Service;
use tracing::warn;

use crate::{service::AddAuthorization, token::TokenSource};

pub struct Client<C, B = Body> {
    inner: AddAuthorization<hyper::Client<C, B>>,
}

#[allow(clippy::new_ret_no_self)]
impl Client<(), Body> {
    pub async fn new<C, B>(client: hyper::Client<C, B>) -> Client<C, B> {
        Client { inner: AddAuthorization::init(client).await }
    }

    pub fn new_with<C, B>(
        client: hyper::Client<C, B>,
        source: impl Into<TokenSource>,
    ) -> Client<C, B> {
        Client { inner: AddAuthorization::init_with(source, client) }
    }
}

impl<C, B> Client<C, B>
where
    C: Connect + Clone + Send + Sync + 'static,
    B: HttpBody + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn error::Error + Send + Sync>>,
{
    pub async fn get(&mut self, uri: hyper::Uri) -> hyper::Result<hyper::Response<Body>>
    where
        B: Default,
    {
        let body = B::default();
        if !body.is_end_stream() {
            warn!("default HttpBody used for get() does not return true for is_end_stream");
        }

        let mut req = Request::new(body);
        *req.uri_mut() = uri;
        self.request(req).await
    }

    pub async fn request(&mut self, req: hyper::Request<B>) -> hyper::Result<Response<Body>> {
        match futures_util::future::poll_fn(|cx| self.inner.poll_ready(cx)).await {
            Ok(()) => self.inner.call(req).await,
            Err(err) => Err(err),
        }
    }
}

impl<C: Clone, B> Clone for Client<C, B> {
    fn clone(&self) -> Self {
        Self { inner: self.inner.clone() }
    }
}

impl<C, B> fmt::Debug for Client<C, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client").finish()
    }
}
