# google-authz

[![ci](https://github.com/mechiru/google-authz/workflows/ci/badge.svg)](https://github.com/mechiru/google-authz/actions?query=workflow:ci)
[![Rust Documentation](https://docs.rs/google-authz/badge.svg)](https://docs.rs/google-authz)
[![Latest Version](https://img.shields.io/crates/v/google-authz.svg)](https://crates.io/crates/google-authz)

This library provides auto-renewed tokens for GCP service authentication.
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


## License

Licensed under either of [Apache License, Version 2.0](./LICENSE-APACHE) or [MIT license](./LICENSE-MIT) at your option.
