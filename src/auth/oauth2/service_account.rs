use std::{fmt, time::SystemTime};

use hyper::Uri;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};

use crate::{
    auth::oauth2::{http::Client, token},
    credentials,
};

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

// https://cloud.google.com/docs/authentication/production
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
    pub(crate) fn new(sa: credentials::ServiceAccount) -> Self {
        Self {
            inner: Client::new(),
            header: header("JWT", sa.private_key_id),
            private_key: EncodingKey::from_rsa_pem(sa.private_key.as_bytes()).unwrap(),
            token_uri: Uri::from_maybe_shared(sa.token_uri.clone()).unwrap(),
            token_uri_str: sa.token_uri,
            scopes: sa.scopes.join(" "),
            client_email: sa.client_email,
        }
    }
}

impl fmt::Debug for ServiceAccount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServiceAccount").finish()
    }
}

impl token::Fetcher for ServiceAccount {
    fn fetch(&self) -> token::ResponseFuture {
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
        Box::pin(self.inner.send(req))
    }
}
