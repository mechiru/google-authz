use std::{
    future::Future,
    pin::Pin,
    time::{Duration, Instant},
};

use hyper::{
    body::{aggregate, Body},
    client::HttpConnector,
    header::{HeaderValue, CONTENT_TYPE, USER_AGENT},
    Method, Request, StatusCode, Uri,
};
use hyper_rustls::HttpsConnector;

use crate::{credentials, token, Credentials};

// === error ===

/// Represents errors that can occur during getting token.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    // internal
    #[error("http client error: {0}")]
    Http(#[from] hyper::Error),
    #[error("gcemeta client error: {0}")]
    Gcemeta(#[from] gcemeta::Error),
    // server
    #[error("response status code error: {0}")]
    StatusCode(StatusCode),
    #[error("json deserialization error: {0:?}")]
    InvalidJson(serde_json::Error),
    #[error("invalid token: {0:?}")]
    InvalidToken(Token),
    #[error("invalid header value: {0:?}")]
    InvalidHeaderValue(hyper::header::InvalidHeaderValue),
}

/// Wrapper for the `Result` type with an [`Error`](Error).
pub type Result<T> = std::result::Result<T, Error>;

// === http ===

struct Client {
    inner: hyper::Client<HttpsConnector<HttpConnector>, Body>,
    user_agent: HeaderValue,
    content_type: HeaderValue,
}

impl Client {
    fn new() -> Client {
        #[allow(unused_variables)]
        #[cfg(feature = "native-certs")]
        let https = HttpsConnector::with_native_roots();
        #[cfg(feature = "webpki-roots")]
        let https = HttpsConnector::with_webpki_roots();

        Client {
            inner: hyper::Client::builder().build(https),
            user_agent: HeaderValue::from_static(concat!(
                "github.com/mechiru/",
                env!("CARGO_PKG_NAME"),
                " v",
                env!("CARGO_PKG_VERSION")
            )),
            content_type: HeaderValue::from_static("application/x-www-form-urlencoded"),
        }
    }

    fn request<T>(&self, uri: &Uri, body: &T) -> Request<Body>
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

    fn send<T>(&self, req: Request<Body>) -> impl Future<Output = Result<T>> + Send + 'static
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
                    serde_json::from_reader(buf.reader()).map_err(Error::InvalidJson)
                }
                code => Err(Error::StatusCode(code)),
            }
        }
    }
}

// === token ===

#[derive(Debug, serde::Deserialize)]
pub struct Token {
    pub token_type: String,
    pub access_token: String,
    pub expires_in: u64,
}

impl Token {
    pub fn into_pairs(self) -> Result<(HeaderValue, Instant)> {
        if self.token_type.is_empty() || self.access_token.is_empty() || self.expires_in == 0 {
            Err(Error::InvalidToken(self))
        } else {
            match HeaderValue::from_str(&format!("{} {}", self.token_type, self.access_token)) {
                Ok(value) => Ok((value, Instant::now() + Duration::from_secs(self.expires_in))),
                Err(err) => Err(Error::InvalidHeaderValue(err)),
            }
        }
    }
}

// === token source ===

pub enum TokenSource {
    User(user::User),
    ServiceAccount(service_account::ServiceAccount),
    Metadata(metadata::Metadata),
}

impl TokenSource {
    pub fn token(&self) -> Pin<Box<dyn Future<Output = token::Result<Token>> + Send + 'static>> {
        match self {
            TokenSource::User(user) => Box::pin(user.token()),
            TokenSource::ServiceAccount(sa) => Box::pin(sa.token()),
            TokenSource::Metadata(meta) => Box::pin(meta.token()),
        }
    }
}

impl From<Credentials> for TokenSource {
    fn from(c: Credentials) -> Self {
        use crate::{
            credentials::Kind,
            token::{service_account as sa, TokenSource::*},
        };
        match c.into_parts() {
            (s, Kind::User(user)) => User(user::User::new(user, s)),
            (s, Kind::ServiceAccount(sa)) => ServiceAccount(sa::ServiceAccount::new(sa, s)),
            (s, Kind::Metadata(meta)) => Metadata(metadata::Metadata::new(meta, s)),
        }
    }
}

pub(super) mod user {
    use super::*;

    #[derive(serde::Serialize)]
    struct Payload<'a> {
        client_id: &'a str,
        client_secret: &'a str,
        grant_type: &'a str,
        refresh_token: &'a str,
    }

    pub struct User {
        inner: Client,
        token_uri: Uri,
        creds: credentials::User,
    }

    impl User {
        pub(crate) fn new(user: credentials::User, _scopes: &'static [&'static str]) -> Self {
            Self {
                inner: Client::new(),
                // https://github.com/golang/oauth2/blob/0f29369cfe4552d0e4bcddc57cc75f4d7e672a33/google/google.go#L24
                token_uri: Uri::from_static("https://oauth2.googleapis.com/token"),
                creds: user,
            }
        }

        pub(crate) fn token(&self) -> impl Future<Output = Result<Token>> + Send + 'static {
            let req = self.inner.request(&self.token_uri, &Payload {
                client_id: &self.creds.client_id,
                client_secret: &self.creds.client_secret,
                grant_type: "refresh_token",
                // The reflesh token is not included in the response from google's server,
                // so it always uses the specified refresh token from the file.
                refresh_token: &self.creds.refresh_token,
            });
            self.inner.send(req)
        }
    }
}

pub(super) mod service_account {
    use std::time::SystemTime;

    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};

    use super::*;

    // If client machine's time is in the future according
    // to Google servers, an access token will not be issued.
    fn issued_at() -> u64 {
        SystemTime::UNIX_EPOCH.elapsed().unwrap().as_secs() - 10
    }

    // https://cloud.google.com/iot/docs/concepts/device-security#security_standards
    fn header(typ: impl Into<String>, key_id: impl Into<String>) -> Header {
        Header {
            typ: Some(typ.into()),
            alg: Algorithm::RS256,
            kid: Some(key_id.into()),
            ..Default::default()
        }
    }

    #[derive(serde::Serialize)]
    struct Claims<'a> {
        iss: &'a str,
        scope: &'a str,
        aud: &'a str,
        iat: u64,
        exp: u64,
    }

    #[derive(serde::Serialize)]
    struct Payload<'a> {
        grant_type: &'a str,
        assertion: &'a str,
    }

    pub struct ServiceAccount {
        inner: Client,
        header: Header,
        private_key: EncodingKey,
        token_uri: Uri,
        token_uri_str: String,
        scopes: String,
        client_email: String,
    }

    impl ServiceAccount {
        pub(crate) fn new(
            sa: credentials::ServiceAccount,
            scopes: &'static [&'static str],
        ) -> Self {
            Self {
                inner: Client::new(),
                header: header("JWT", sa.private_key_id),
                private_key: EncodingKey::from_rsa_pem(sa.private_key.as_bytes()).unwrap(),
                token_uri: Uri::from_maybe_shared(sa.token_uri.clone()).unwrap(),
                token_uri_str: sa.token_uri,
                scopes: scopes.join(" "),
                client_email: sa.client_email,
            }
        }

        pub(crate) fn token(&self) -> impl Future<Output = Result<Token>> + Send + 'static {
            const EXPIRE: u64 = 60 * 60;

            let iat = issued_at();
            let claims = Claims {
                iss: &self.client_email,
                scope: &self.scopes,
                aud: &self.token_uri_str,
                iat,
                exp: iat + EXPIRE,
            };

            let req = self.inner.request(&self.token_uri, &Payload {
                grant_type: "urn:ietf:params:oauth:grant-type:jwt-bearer",
                assertion: &encode(&self.header, &claims, &self.private_key).unwrap(),
            });
            self.inner.send(req)
        }
    }
}

pub(super) mod metadata {
    use std::str::FromStr;

    use hyper::{client::HttpConnector, http::uri::PathAndQuery, Body};

    use super::*;

    #[derive(serde::Serialize)]
    struct Query<'a> {
        scopes: &'a str,
    }

    pub struct Metadata {
        inner: gcemeta::Client<HttpConnector, Body>,
        path_and_query: PathAndQuery,
    }

    impl Metadata {
        pub(crate) fn new(meta: credentials::Metadata, scopes: &'static [&'static str]) -> Self {
            let query = match scopes.len() {
                0 => String::new(),
                _ => serde_urlencoded::to_string(&Query { scopes: &scopes.join(",") }).unwrap(),
            };
            let path_and_query = format!(
                "/computeMetadata/v1/instance/service-accounts/{}/token?{}",
                meta.account.unwrap_or("default"),
                query
            );
            let path_and_query = PathAndQuery::from_str(&path_and_query).unwrap();
            Self { inner: meta.client, path_and_query }
        }

        pub fn token(&self) -> impl Future<Output = Result<Token>> + Send + 'static {
            // Already checked that this process is running on GCE.
            let fut = self.inner.get_as(self.path_and_query.clone());
            async { Ok(fut.await?) }
        }
    }
}
