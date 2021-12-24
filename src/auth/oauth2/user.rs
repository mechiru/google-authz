use std::fmt;

use hyper::Uri;

use crate::{
    auth::oauth2::{http::Client, token},
    credentials,
};

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
    credentials: credentials::User,
}

impl User {
    pub(crate) fn new(user: credentials::User) -> Self {
        Self {
            inner: Client::new(),
            // https://github.com/golang/oauth2/blob/0f29369cfe4552d0e4bcddc57cc75f4d7e672a33/google/google.go#L24
            token_uri: Uri::from_static("https://oauth2.googleapis.com/token"),
            credentials: user,
        }
    }
}

impl fmt::Debug for User {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("User").finish()
    }
}

impl token::Fetcher for User {
    fn fetch(&self) -> token::ResponseFuture {
        let req = self.inner.request(&self.token_uri, &Payload {
            client_id: &self.credentials.client_id,
            client_secret: &self.credentials.client_secret,
            grant_type: "refresh_token",
            // The reflesh token is not included in the response from google's server,
            // so it always uses the specified refresh token from the file.
            refresh_token: &self.credentials.refresh_token,
        });
        Box::pin(self.inner.send(req))
    }
}
