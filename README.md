# google-authz

[![ci](https://github.com/mechiru/google-authz/workflows/ci/badge.svg)](https://github.com/mechiru/google-authz/actions?query=workflow:ci)
[![Rust Documentation](https://docs.rs/google-authz/badge.svg)](https://docs.rs/google-authz)
[![Latest Version](https://img.shields.io/crates/v/google-authz.svg)](https://crates.io/crates/google-authz)

This library provides auto-renewed tokens for GCP service authentication.<br>
**google-authz = tower-service + gcp authentication**

## Notes

| Authentication flow                  | Status                              |
|--------------------------------------|-------------------------------------|
| API key                              | Not supported / No plans to support |
| OAuth 2.0 client                     | Supported                           |
| Environment-provided service account | Supported                           |
| Service account key                  | Supported                           |


## Example

### Default

- Scope is `https://www.googleapis.com/auth/cloud-platform`
- Looks for credentials in the following places, preferring the first location found:
  - A JSON file whose path is specified by the `GOOGLE_APPLICATION_CREDENTIALS` environment variable.
  - A JSON file in a location known to the gcloud command-line tool.
  - On Google Compute Engine, it fetches credentials from the metadata server.

```rust
let creds = Credentials::default().await;
let service = AddAuthorization::init_with(creds, service);

// same as above
let service = AddAuthorization::init(service).await;
```


### Custom

scope:
```rust
let creds = Credentials::find_default(scopes).await;
let service = AddAuthorization::init_with(creds, service);
```

json:
```rust
let creds = Credentials::from_json(json, scopes);
let service = AddAuthorization::init_with(creds, service);
```

file:
```rust
let creds = Credentials::from_file(path, scopes);
let service = AddAuthorization::init_with(creds, 
```

### with [tonic](github.com/hyperium/tonic)

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt::init();

    let project = env::args().nth(1).expect("cargo run --bin tonic -- <GCP_PROJECT_ID>");

    let tls_config = ClientTlsConfig::new()
        .ca_certificate(Certificate::from_pem(CERTIFICATES))
        .domain_name("pubsub.googleapis.com");

    let channel = Channel::from_static("https://pubsub.googleapis.com")
        .tls_config(tls_config)?
        .connect()
        .await?;

    let channel = AddAuthorization::init(channel).await;

    let mut client = PublisherClient::new(channel);
    let resp = client
        .list_topics(Request::new(ListTopicsRequest {
            project: format!("projects/{}", project),
            page_size: 10,
            ..Default::default()
        }))
        .await?;
    println!("response = {:?}", resp);

    Ok(())
}
```

The complete code can be found [here](./examples/src/tonic.rs).

### with [hyper](https://github.com/hyperium/hyper)

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt::init();

    let project = env::args().nth(1).expect("cargo run --bin hyper -- <GCP_PROJECT_ID>");

    let https = HttpsConnector::with_native_roots();
    let client = hyper::Client::builder().build::<_, Body>(https);
    let mut client = Client::new(client).await;

    let uri = Uri::try_from(format!(
        "https://pubsub.googleapis.com/v1/projects/{}/topics?alt=json&prettyPrint=true",
        project
    ))?;
    let (parts, body) = client.get(uri).await?.into_parts();
    println!("response parts = {:?}", parts);

    let body = String::from_utf8(to_bytes(body).await?.to_vec())?;
    println!("resposne body = `{}`", body);

    Ok(())
}
```

The complete code can be found [here](./examples/src/hyper.rs).


## License

Licensed under either of [Apache License, Version 2.0](./LICENSE-APACHE) or [MIT license](./LICENSE-MIT) at your option.
