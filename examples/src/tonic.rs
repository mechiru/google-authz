use googapis::{
    google::pubsub::v1::{publisher_client::PublisherClient, ListTopicsRequest},
    CERTIFICATES,
};
use google_authz::AddAuthorization;
use tonic::{
    transport::{Certificate, Channel, ClientTlsConfig},
    Request,
};

use std::{env, error::Error};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt::init();

    let project = env::args().nth(1).expect("cargo run --example tonic -- <GCP_PROJECT_ID>");

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
