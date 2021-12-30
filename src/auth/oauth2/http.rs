use std::future::Future;

use hyper::{
    body::aggregate,
    client::HttpConnector,
    header::{HeaderValue, CONTENT_TYPE, USER_AGENT},
    Body, Method, Request, StatusCode, Uri,
};
use hyper_rustls::{builderstates::WantsSchemes, HttpsConnector, HttpsConnectorBuilder};

use crate::auth;

pub(super) struct Client {
    inner: hyper::Client<HttpsConnector<HttpConnector>, Body>,
    user_agent: HeaderValue,
    content_type: HeaderValue,
}

impl Client {
    pub fn new() -> Client {
        let https = connection_builder().https_only().enable_http2().build();
        let user_agent =
            concat!("github.com/mechiru/", env!("CARGO_PKG_NAME"), " v", env!("CARGO_PKG_VERSION"));
        Self {
            inner: hyper::Client::builder().build(https),
            user_agent: HeaderValue::from_static(user_agent),
            content_type: HeaderValue::from_static("application/x-www-form-urlencoded"),
        }
    }

    pub fn request<T>(&self, uri: &Uri, body: &T) -> Request<Body>
    where
        T: serde::Serialize,
    {
        let mut req = Request::builder().uri(uri).method(Method::POST);
        let headers = req.headers_mut().unwrap();
        headers.insert(USER_AGENT, self.user_agent.clone());
        headers.insert(CONTENT_TYPE, self.content_type.clone());
        let body = Body::from(serde_urlencoded::to_string(body).unwrap());
        req.body(body).unwrap()
    }

    pub fn send<T>(
        &self,
        req: Request<Body>,
    ) -> impl Future<Output = auth::Result<T>> + Send + 'static
    where
        T: serde::de::DeserializeOwned,
    {
        let fut = self.inner.request(req);
        async {
            use bytes::Buf as _;

            let (parts, body) = fut.await?.into_parts();
            match parts.status {
                StatusCode::OK => {
                    let buf = aggregate(body).await?;
                    serde_json::from_reader(buf.reader()).map_err(auth::Error::JsonDeserialize)
                }
                _ => Err(auth::Error::StatusCode((parts, body))),
            }
        }
    }
}

#[cfg(feature = "native-certs")]
fn connection_builder() -> HttpsConnectorBuilder<WantsSchemes> {
    HttpsConnectorBuilder::new().with_native_roots()
}

#[cfg(all(not(feature = "native-certs"), feature = "webpki-roots"))]
fn connection_builder() -> HttpsConnectorBuilder<WantsSchemes> {
    HttpsConnectorBuilder::new().with_webpki_roots()
}
