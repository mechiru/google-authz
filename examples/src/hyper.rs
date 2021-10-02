use std::{convert::TryFrom, env, error::Error};

use google_authz::Client;
use hyper::{body::to_bytes, Body, Uri};
use hyper_rustls::HttpsConnector;

// https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.topics/list
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
