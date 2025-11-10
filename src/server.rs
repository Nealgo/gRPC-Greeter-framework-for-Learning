use tokio::sync::mpsc;
use tonic::{Request, Response, Status, transport::Server};
use tracing::info;
use tracing_subscriber::{fmt::format::FmtSpan, self};

use hello_world::greeter_server::{Greeter, GreeterServer};
use hello_world::{HelloReply, HelloRequest};

use tokio::time::{Duration, sleep};
use tokio_stream::wrappers::ReceiverStream;

pub mod hello_world {
    tonic::include_proto!("helloworld");
}

#[derive(Debug, Default)]
pub struct MyGreeter {}

#[tonic::async_trait]
impl Greeter for MyGreeter {
    type SayHelloStreamStream = ReceiverStream<Result<HelloReply, Status>>;

    #[tracing::instrument]
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        info!("Got a request: {:?}", request);
        let name = request.into_inner().name;

        // Simulate some business logic
        tracing::info_span!("server_processing").in_scope(|| async {
            sleep(Duration::from_millis(20)).await;
        }).await;

        let reply = HelloReply {
            message: format!("Hello {}!", name),
        };

        Ok(Response::new(reply))
    }

    // server-streaming RPC
    #[tracing::instrument]
    async fn say_hello_stream(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<Self::SayHelloStreamStream>, Status> {
        let name = request.into_inner().name;

        let (tx, rx) = mpsc::channel(4);

        tokio::spawn(async move {
            let greetings = vec![
                format!("Hello, {}! (1/3)", name),
                format!("Hi again, {}! (2/3)", name),
                format!("Greetings, {}! (3/3)", name),
            ];

            for greeting in greetings {
                if tx.send(Ok(HelloReply { message: greeting })).await.is_err() {
                    break;
                }
                sleep(Duration::from_secs(1)).await;
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_span_events(FmtSpan::FULL)
        .with_current_span(true)
        .with_span_list(true)
        .init();

    let addr = "[::1]:50051".parse()?;
    let greeter = MyGreeter::default();

    info!("Server listening on {}", addr);

    Server::builder()
        .add_service(GreeterServer::new(greeter))
        .serve(addr)
        .await?;

    Ok(())
}
