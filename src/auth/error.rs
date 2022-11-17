/// Represents errors that can occur during fetching token.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("gcemeta client error: {0}")]
    Gcemeta(#[from] gcemeta::Error),
    #[error("http client error: {0}")]
    Http(#[from] hyper::Error),
    #[error("response status code error: {0:?}")]
    StatusCode((hyper::http::response::Parts, hyper::Body)),
    #[error("response body deserialize error: {0}")]
    JsonDeserialize(serde_json::Error),
    #[error("token format error: {0:?}")]
    TokenFormat(crate::auth::oauth2::token::Response),
    #[error("invalid uri: {0}")]
    InvalidUri(#[from] hyper::http::uri::InvalidUri),
    #[error("invalid uri parts: {0}")]
    InvalidUriParts(#[from] hyper::http::uri::InvalidUriParts),
    #[cfg(not(feature = "tonic"))]
    #[error("uri schema error: {0:?}")]
    EnforceHttps(Option<String>),
}

/// Wrapper for the `Result` type with an [`Error`](Error).
pub(crate) type Result<T> = std::result::Result<T, Error>;

/// Represents errors that can occur while building an Auth channel.
#[derive(thiserror::Error, Debug)]
pub enum AuthBuilderError {
    #[error("encoding key error: {0}")]
    EncodingKey(#[from] jsonwebtoken::errors::Error),
    #[error("invalid uri: {0}")]
    InvalidUri(#[from] hyper::http::uri::InvalidUri),
}
