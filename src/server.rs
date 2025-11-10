use tokio::sync::mpsc;
use tonic::{Request, Response, Status, transport::Server};

use hello_world::greeter_server::{Greeter, GreeterServer};
use hello_world::{HelloReply, HelloRequest};

use tokio::time::{Duration, sleep};
use tokio_stream::wrappers::ReceiverStream;

use std::time::Instant;
use std::env;

pub mod hello_world {
    tonic::include_proto!("helloworld");
}

#[derive(Debug, Default)]
pub struct MyGreeter {}

#[tonic::async_trait]
impl Greeter for MyGreeter {
    type SayHelloStreamStream = ReceiverStream<Result<HelloReply, Status>>;

    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        println!("Got a request: {:?}", request);
        let name = request.into_inner().name;
        let reply = HelloReply {
            message: format!("Hello {}!", name),
        };

        Ok(Response::new(reply))
    }

    // server-streaming RPC
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

use hello_world::greeter_client::GreeterClient;

async fn run_benchmark(addr: &str, iterations: usize, payload_size: usize) -> Result<(), Box<dyn std::error::Error>> {
    // connect to server
    let uri = if addr.starts_with('[') {
        format!("http://{}", addr) // handles [::1]:50051
    } else {
        format!("http://{}", addr)
    };

    let mut client = GreeterClient::connect(uri).await?;

    // prepare payload ~ payload_size bytes
    let base = "a";
    let mut payload = String::new();
    while payload.len() < payload_size {
        payload.push_str(base);
    }
    payload.truncate(payload_size);

    let mut latencies_us: Vec<u128> = Vec::with_capacity(iterations);

    let overall_start = Instant::now();
    for _ in 0..iterations {
        let req = HelloRequest { name: payload.clone() };
        let start = Instant::now();
        let _resp = client.say_hello(Request::new(req)).await?;
        let elapsed = start.elapsed();
        latencies_us.push(elapsed.as_micros());
    }
    let total_elapsed = overall_start.elapsed();

    // compute stats
    latencies_us.sort_unstable();
    let n = latencies_us.len();

    if n == 0 {
        println!("No iterations run, cannot compute stats.");
        return Ok(());
    }

    let p50 = latencies_us[((n as f64 * 0.50).floor() as usize).saturating_sub(1)];
    let p90 = latencies_us[((n as f64 * 0.90).floor() as usize).saturating_sub(1)];
    let p99 = latencies_us[((n as f64 * 0.99).floor() as usize).saturating_sub(1)];
    let sum: u128 = latencies_us.iter().sum();
    let mean_us = sum / (n as u128);
    let rps = (n as f64) / total_elapsed.as_secs_f64();

    println!("Benchmark results:");
    println!("  iterations: {}", n);
    println!("  total time: {:.3}s", total_elapsed.as_secs_f64());
    println!("  RPS: {:.1}", rps);
    println!("  mean: {} μs ({:.3} ms)", mean_us, mean_us as f64 / 1000.0);
    println!("  p50: {} μs ({:.3} ms)", p50, p50 as f64 / 1000.0);
    println!("  p90: {} μs ({:.3} ms)", p90, p90 as f64 / 1000.0);
    println!("  p99: {} μs ({:.3} ms)", p99, p99 as f64 / 1000.0);

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr: std::net::SocketAddr = "[::1]:50051".parse()?;
    let greeter = MyGreeter::default();

    // Simple CLI: --bench to run benchmark (will spawn server in-process),
    // optional --iterations=N and --payload-size=M
    let args: Vec<String> = env::args().collect();
    let is_bench = args.iter().any(|a| a == "--bench");
    let mut iterations = 2000usize;
    let mut payload_size = 1024usize;

    for arg in &args {
        if let Some(v) = arg.strip_prefix("--iterations=") {
            if let Ok(x) = v.parse() { iterations = x; }
        }
        if let Some(v) = arg.strip_prefix("--payload-size=") {
            if let Ok(x) = v.parse() { payload_size = x; }
        }
    }

    if is_bench {
        println!("Spawning server and running benchmark against {}", addr);
        // spawn server in background
        let srv_addr = addr.clone();
        let srv_greeter = greeter;
        tokio::spawn(async move {
            if let Err(e) = Server::builder()
                .add_service(GreeterServer::new(srv_greeter))
                .serve(srv_addr)
                .await
            {
                eprintln!("server error: {}", e);
            }
        });

        // give server a short moment to start
        tokio::time::sleep(Duration::from_millis(200)).await;

        let addr_str = "[::1]:50051";
        run_benchmark(addr_str, iterations, payload_size).await?;
        return Ok(());
    }

    println!("Server listening on {}", addr);

    Server::builder()
        .add_service(GreeterServer::new(greeter))
        .serve(addr)
        .await?;

    Ok(())
}
