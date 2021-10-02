use std::{
    env, fs,
    path::{Path, PathBuf},
};

use hyper::{client::HttpConnector, Body};
use tracing::trace;

/// Represents errors that can occur during finding credentials.
#[derive(thiserror::Error, Debug)]
enum Error {
    // internal
    #[error("gcemeta client error: {0}")]
    Gcemeta(#[from] gcemeta::Error),
    // user
    #[error("not found credentials")]
    NotFound,
    #[error("read file error: {0}")]
    ReadFile(std::io::Error),
    #[error("failed deserialize to user or service account credentials")]
    InvalidCredentials,
}

/// Wrapper for the `Result` type with an [`Error`](Error).
type Result<T> = std::result::Result<T, Error>;

/// Looks for credentials in the following places, preferring the first location found:
/// - A JSON file whose path is specified by the `GOOGLE_APPLICATION_CREDENTIALS` environment variable.
/// - A JSON file in a location known to the gcloud command-line tool.
/// - On Google Compute Engine, it fetches credentials from the metadata server.
async fn find_default(scopes: Option<&'static [&'static str]>) -> Result<Credentials> {
    let scopes = scopes.unwrap_or(&["https://www.googleapis.com/auth/cloud-platform"]);

    let creds = if let Some(creds) = from_env_var(scopes)? {
        creds
    } else if let Some(creds) = from_well_known_file(scopes)? {
        creds
    } else if let Some(creds) = from_metadata(None, scopes).await? {
        creds
    } else {
        return Err(Error::NotFound);
    };

    Ok(creds)
}

fn from_env_var(scopes: &'static [&'static str]) -> Result<Option<Credentials>> {
    const NAME: &str = "GOOGLE_APPLICATION_CREDENTIALS";
    trace!("try getting `{}` from environment variable", NAME);
    match env::var(NAME) {
        Ok(path) => from_file(path, scopes).map(Some),
        Err(err) => {
            trace!("failed to get environment variable: {:?}", err);
            Ok(None)
        }
    }
}

fn from_well_known_file(scopes: &'static [&'static str]) -> Result<Option<Credentials>> {
    let path = {
        let mut buf = {
            #[cfg(target_os = "windows")]
            {
                PathBuf::from(env::var("APPDATA").unwrap_or_default())
            }
            #[cfg(not(target_os = "windows"))]
            {
                let mut buf = PathBuf::from(env::var("HOME").unwrap_or_default());
                buf.push(".config");
                buf
            }
        };

        buf.push("gcloud");
        buf.push("application_default_credentials.json");
        buf
    };

    trace!("well known file path is {:?}", path);
    match path.exists() {
        true => from_file(path, scopes).map(Some),
        false => {
            trace!("no file exists at {:?}", path);
            Ok(None)
        }
    }
}

async fn from_metadata(
    account: Option<&'static str>,
    scopes: &'static [&'static str],
) -> Result<Option<Credentials>> {
    let client = gcemeta::Client::new();

    trace!("try checking if this process is running on GCE");
    let on = client.on_gce().await?;
    trace!("this process is running on GCE: {}", on);

    if on {
        Ok(Some(Credentials { scopes, kind: Kind::Metadata(Metadata { client, account }) }))
    } else {
        Ok(None)
    }
}

fn from_file(path: impl AsRef<Path>, scopes: &'static [&'static str]) -> Result<Credentials> {
    trace!("try reading credentials file from {:?}", path.as_ref());
    let buf = fs::read_to_string(path).map_err(Error::ReadFile)?;
    from_json(buf.as_bytes(), scopes)
}

fn from_json(buf: &[u8], scopes: &'static [&'static str]) -> Result<Credentials> {
    trace!("try deserializing to service account credentials");
    match serde_json::from_slice(buf) {
        Ok(sa) => return Ok(Credentials { scopes, kind: Kind::ServiceAccount(sa) }),
        Err(err) => trace!("failed deserialize to service account credentials: {:?}", err),
    }

    trace!("try deserializing to user credentials");
    match serde_json::from_slice(buf) {
        Ok(user) => return Ok(Credentials { scopes, kind: Kind::User(user) }),
        Err(err) => trace!("failed deserialize to user credentials: {:?}", err),
    }

    Err(Error::InvalidCredentials)
}

// https://cloud.google.com/iam/docs/creating-managing-service-account-keys
#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug)]
pub struct Credentials {
    scopes: &'static [&'static str],
    kind: Kind,
}

impl Credentials {
    pub async fn default() -> Self {
        find_default(None).await.unwrap()
    }

    pub async fn find_default(scopes: Option<&'static [&'static str]>) -> Self {
        find_default(scopes).await.unwrap()
    }

    pub fn from_json(json: &[u8], scopes: &'static [&'static str]) -> Self {
        from_json(json, scopes).unwrap()
    }

    pub fn from_file(path: impl AsRef<Path>, scopes: &'static [&'static str]) -> Self {
        from_file(path, scopes).unwrap()
    }

    pub async fn from_metadata(
        account: Option<&'static str>,
        scopes: &'static [&'static str],
    ) -> Self {
        match from_metadata(account, scopes).await.unwrap() {
            Some(creds) => creds,
            None => panic!("this process is not running on GCE"),
        }
    }

    pub(crate) fn into_parts(self) -> (&'static [&'static str], Kind) {
        (self.scopes, self.kind)
    }
}

#[allow(clippy::large_enum_variant)]
#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug)]
pub(crate) enum Kind {
    User(User),
    ServiceAccount(ServiceAccount),
    Metadata(Metadata),
}

#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug, serde::Deserialize)]
pub(crate) struct User {
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
}

#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug, serde::Deserialize)]
pub(crate) struct ServiceAccount {
    pub client_email: String,
    pub private_key_id: String,
    pub private_key: String,
    pub token_uri: String,
}

#[derive(Debug)]
pub(crate) struct Metadata {
    pub client: gcemeta::Client<HttpConnector, Body>,
    pub account: Option<&'static str>,
}

#[cfg(test)]
impl PartialEq for Metadata {
    fn eq(&self, other: &Self) -> bool {
        self.account == other.account
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const SA: &[u8] = br#"{
"type": "service_account",
"project_id": "[PROJECT-ID]",
"private_key_id": "[KEY-ID]",
"private_key": "-----BEGIN PRIVATE KEY-----\n[PRIVATE-KEY]\n-----END PRIVATE KEY-----\n",
"client_email": "[SERVICE-ACCOUNT-EMAIL]",
"client_id": "[CLIENT-ID]",
"auth_uri": "https://accounts.google.com/o/oauth2/auth",
"token_uri": "https://accounts.google.com/o/oauth2/token",
"auth_provider_x509_cert_url": "https://www.googleapis.com/oauth2/v1/certs",
"client_x509_cert_url": "https://www.googleapis.com/robot/v1/metadata/x509/[SERVICE-ACCOUNT-EMAIL]"
}"#;

    const USER: &[u8] = br#"{
  "client_id": "xxx.apps.googleusercontent.com",
  "client_secret": "secret-xxx",
  "refresh_token": "refresh-xxx",
  "type": "authorized_user"
}"#;
    #[test]
    fn test_from_json() -> Result<()> {
        assert_eq!(from_json(SA, &[])?, Credentials {
            scopes: &[],
            kind: Kind::ServiceAccount(ServiceAccount {
                client_email: "[SERVICE-ACCOUNT-EMAIL]".into(),
                private_key_id: "[KEY-ID]".into(),
                private_key:
                    "-----BEGIN PRIVATE KEY-----\n[PRIVATE-KEY]\n-----END PRIVATE KEY-----\n".into(),
                token_uri: "https://accounts.google.com/o/oauth2/token".into(),
            })
        });

        assert_eq!(from_json(USER, &[])?, Credentials {
            scopes: &[],
            kind: Kind::User(User {
                client_id: "xxx.apps.googleusercontent.com".into(),
                client_secret: "secret-xxx".into(),
                refresh_token: "refresh-xxx".into(),
            })
        });

        Ok(())
    }
}
