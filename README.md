# google-authz

[![ci](https://github.com/mechiru/google-authz/workflows/ci/badge.svg)](https://github.com/mechiru/google-authz/actions?query=workflow:ci)
[![pub](https://github.com/mechiru/google-authz/workflows/pub/badge.svg)](https://github.com/mechiru/google-authz/actions?query=workflow:pub)
[![doc](https://docs.rs/google-authz/badge.svg)](https://docs.rs/google-authz)
[![version](https://img.shields.io/crates/v/google-authz.svg)](https://crates.io/crates/google-authz)

This library provides auto-renewed tokens for GCP service authentication.<br>
**google-authz = tower-service + gcp authentication**

## Notes

| Authentication flow                  | Status    |
|--------------------------------------|-----------|
| API key                              | Supported |
| OAuth 2.0 client                     | Supported |
| Environment-provided service account | Supported |
| Service account key                  | Supported |


## Example

### Default

- Scope is `https://www.googleapis.com/auth/cloud-platform`
- Looks for credentials in the following places, preferring the first location found:
  - A JSON file whose path is specified by the `GOOGLE_APPLICATION_CREDENTIALS` environment variable.
  - A JSON file in a location known to the gcloud command-line tool.
  - On Google Compute Engine, it fetches credentials from the metadata server.

```rust
use google_authz::{Credentials, GoogleAuthz};

let credentials = Credentials::builder().build().await.unwrap();
let service = GoogleAuthz::builder(service).credentials(credentials).build().await;

// same as above
let service = GoogleAuthz::new(service).await;
```



### Custom

no auth:
```rust
let credentials = Credentials::builder().no_credentials().build().await.unwrap();
let service = GoogleAuthz::builder(service).credentials(credentials).build().await;
```

api key:
```rust
let credentials = Credentials::builder().api_key(api_key).build().await.unwrap();
let service = GoogleAuthz::builder(service).credentials(credentials).build().await;
```

json:
```rust
let credentials = Credentials::builder().json(json).build().await.unwrap();
let service = GoogleAuthz::builder(service).credentials(credentials).build().await;
```

json file:
```rust
let credentials = Credentials::builder().json_file(json_file).build().await.unwrap();
let service = GoogleAuthz::builder(service).credentials(credentials).build().await;
```

metadata:
```rust
let credentials = Credentials::builder().metadata(None).build().await.unwrap();
let service = GoogleAuthz::builder(service).credentials(credentials).build().await;
```

scope:
```rust
let credentials = Credentials::builder().scopes(scopes).build().await.unwrap();
let service = GoogleAuthz::builder(service).credentials(credentials).build().await;
```


### with [tonic](github.com/hyperium/tonic)

**When using with tonic crate, please enable the `tonic` feature.**

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let project = env::args().nth(1).expect("cargo run --bin tonic -- <GCP_PROJECT_ID>");
    let channel = Channel::from_static("https://pubsub.googleapis.com").connect().await?;
    let channel = GoogleAuthz::new(channel).await;

    let mut client = PublisherClient::new(channel);
    let response = client
        .list_topics(Request::new(ListTopicsRequest {
            project: format!("projects/{}", project),
            page_size: 10,
            ..Default::default()
        }))
        .await?;
    println!("response = {:#?}", response);

    Ok(())
}
```

The complete code can be found [here](./examples/src/tonic.rs).



## License

Licensed under either of [Apache License, Version 2.0](./LICENSE-APACHE) or [MIT license](./LICENSE-MIT) at your option.
