use std::{fmt, str::FromStr as _};

use futures_util::TryFutureExt as _;
use hyper::{client::HttpConnector, http::uri::PathAndQuery, Body};

use crate::{
    auth::{self, oauth2::token},
    credentials,
};

#[derive(serde::Serialize)]
struct Query<'a> {
    scopes: &'a str,
}

pub struct Metadata {
    inner: gcemeta::Client<HttpConnector, Body>,
    path_and_query: PathAndQuery,
}

impl Metadata {
    pub(crate) fn new(meta: Box<credentials::Metadata>) -> Self {
        let mut path_and_query = "/computeMetadata/v1/instance/service-accounts/".to_owned();
        path_and_query.push_str(meta.account.as_ref().map_or("default", String::as_str));
        path_and_query.push_str("/token");
        if !meta.scopes.is_empty() {
            path_and_query.push('?');
            let query = Query { scopes: &meta.scopes.join(",") };
            path_and_query.push_str(&serde_urlencoded::to_string(&query).unwrap());
        }

        let path_and_query = PathAndQuery::from_str(&path_and_query).unwrap();
        Self { inner: meta.client, path_and_query }
    }
}

impl fmt::Debug for Metadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Metadata").finish()
    }
}

impl token::Fetcher for Metadata {
    fn fetch(&self) -> token::ResponseFuture {
        // Already checked that this process is running on GCE.
        let fut = self.inner.get_as(self.path_and_query.clone()).map_err(auth::Error::Gcemeta);
        Box::pin(fut)
    }
}
