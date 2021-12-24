/// Represents errors that can occur during finding credentials.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("gcemeta client error: {0}")]
    Gcemeta(#[from] gcemeta::Error),
    #[error("api key format error: {0}")]
    ApiKeyFormat(hyper::http::uri::InvalidUri),
    #[error(
        "not found credentials source, please set the environment variable `RUST_LOG` to `google_authz=trace` for more details"
    )]
    CredentialsSource,
    #[error("read credentials file error: {0}")]
    CredentialsFile(std::io::Error),
    #[error(
        "user or service account credentials format error: user={user}, service_account={service_account})"
    )]
    CredentialsFormat { user: serde_json::Error, service_account: serde_json::Error },
}

/// Wrapper for the `Result` type with an [`Error`](Error).
pub type Result<T> = std::result::Result<T, Error>;
