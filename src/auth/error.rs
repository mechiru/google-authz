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
    #[cfg(not(feature = "tonic"))]
    #[error("uri schema error: {0:?}")]
    EnforceHttps(Option<String>),
}

/// Wrapper for the `Result` type with an [`Error`](Error).
pub(crate) type Result<T> = std::result::Result<T, Error>;
