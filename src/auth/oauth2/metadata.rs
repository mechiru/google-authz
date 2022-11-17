use std::{fmt, str::FromStr as _};

use futures_util::TryFutureExt as _;
use hyper::{client::HttpConnector, http::uri::PathAndQuery, Body};

use crate::{
    auth::{self, oauth2::token, AuthBuilderError},
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
    pub(crate) fn try_new(meta: Box<credentials::Metadata>) -> Result<Self, AuthBuilderError> {
        let path_and_query = path_and_query(meta.account, meta.scopes);
        let path_and_query = PathAndQuery::from_str(&path_and_query)?;
        Ok(Self {
            inner: meta.client,
            path_and_query,
        })
    }
}

fn path_and_query(account: Option<String>, scopes: &'static [&'static str]) -> String {
    let mut path_and_query = "/computeMetadata/v1/instance/service-accounts/".to_owned();
    path_and_query.push_str(account.as_ref().map_or("default", String::as_str));
    path_and_query.push_str("/token");
    if !scopes.is_empty() {
        path_and_query.push('?');
        let query = Query {
            scopes: &scopes.join(","),
        };
        path_and_query.push_str(&serde_urlencoded::to_string(&query).unwrap());
    }
    path_and_query
}

impl fmt::Debug for Metadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Metadata").finish()
    }
}

impl token::Fetcher for Metadata {
    fn fetch(&self) -> token::ResponseFuture {
        // Already checked that this process is running on GCE.
        let fut = self
            .inner
            .get_as(self.path_and_query.clone())
            .map_err(auth::Error::Gcemeta);
        Box::pin(fut)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_path_and_query() {
        assert_eq!(
            &path_and_query(None, &[]),
            "/computeMetadata/v1/instance/service-accounts/default/token"
        );

        assert_eq!(
            &path_and_query(None, &["https://www.googleapis.com/auth/cloud-platform"]),
            "/computeMetadata/v1/instance/service-accounts/default/token?scopes=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fcloud-platform"
        );

        assert_eq!(
            &path_and_query(None, &["scope1", "scope2"]),
            "/computeMetadata/v1/instance/service-accounts/default/token?scopes=scope1%2Cscope2"
        );
    }
}
