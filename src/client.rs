use hello_world::greeter_client::GreeterClient;
use hello_world::HelloRequest;
use tracing::{info, info_span};
use tracing_subscriber::{fmt::format::FmtSpan, self};

pub mod hello_world {
    tonic::include_proto!("helloworld");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_span_events(FmtSpan::FULL)
        .with_current_span(true)
        .with_span_list(true)
        .init();

    let mut client = GreeterClient::connect("http://[::1]:50051").await?;

    for i in 0..10 {
        let mut client_clone = client.clone();
        let request = tonic::Request::new(HelloRequest {
            name: format!("Alice-{}", i),
        });

        let response = info_span!("client_call", iteration = i).in_scope(|| async {
            client_clone.say_hello(request).await
        }).await?;

        info!("Unary Response: {}", response.into_inner().message);
    }

    Ok(())
}

