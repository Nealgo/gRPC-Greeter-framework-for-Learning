
use clap::Parser;
use hdrhistogram::Histogram;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, Mutex};
use tokio::time;
use tonic::transport::Channel;

pub mod hello_world {
    tonic::include_proto!("helloworld");
}

use hello_world::{greeter_client::GreeterClient, HelloRequest};

#[derive(Parser, Debug)]
#[clap(name = "benchmark-client", about = "A gRPC benchmark client.")]
struct Opts {
    #[clap(long, default_value = "http://[::1]:50051")]
    server_addr: String,

    #[clap(long, default_value = "100")]
    concurrency: u32,

    #[clap(long, default_value = "10")]
    duration_sec: u64,

    #[clap(long, default_value = "1024")]
    payload_size: usize,

    #[clap(long, default_value = "unary")]
    rpc_type: String,
}

async fn send_unary_request(client: &mut GreeterClient<Channel>, payload: &str) -> Result<(), tonic::Status> {
    let request = tonic::Request::new(HelloRequest {
        name: payload.to_string(),
    });
    client.say_hello(request).await?;
    Ok(())
}

async fn send_streaming_request(client: &mut GreeterClient<Channel>, payload: &str) -> Result<(), tonic::Status> {
    let request = tonic::Request::new(HelloRequest {
        name: payload.to_string(),
    });
    let mut stream = client.say_hello_stream(request).await?.into_inner();
    while let Some(_) = stream.message().await? {}
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opts = Opts::parse();

    let histogram = Arc::new(Mutex::new(Histogram::<u64>::new(3).unwrap()));
    let total_requests = Arc::new(Mutex::new(0u64));
    let payload = "a".repeat(opts.payload_size);

    let (shutdown_tx, _) = broadcast::channel::<()>(1);
    let mut tasks = vec![];

    for _ in 0..opts.concurrency {
        let server_addr = opts.server_addr.clone();
        let rpc_type = opts.rpc_type.clone();
        let payload = payload.clone();
        let histogram = histogram.clone();
        let total_requests = total_requests.clone();
        let mut shutdown_rx = shutdown_tx.subscribe();

        tasks.push(tokio::spawn(async move {
            let mut client = GreeterClient::connect(server_addr).await.unwrap();
            
            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        break;
                    }
                    _ = async {
                        let start = Instant::now();
                        let result = if rpc_type == "unary" {
                            send_unary_request(&mut client, &payload).await
                        } else {
                            send_streaming_request(&mut client, &payload).await
                        };

                        if result.is_ok() {
                            let elapsed = start.elapsed().as_micros() as u64;
                            let mut hist = histogram.lock().await;
                            hist.record(elapsed).unwrap();
                            let mut total = total_requests.lock().await;
                            *total += 1;
                        } else {
                            // You might want to log errors here
                        }
                    } => {}
                }
            }
        }));
    }

    println!(
        "Starting benchmark with {} concurrency for {} seconds...",
        opts.concurrency, opts.duration_sec
    );
    time::sleep(Duration::from_secs(opts.duration_sec)).await;

    shutdown_tx.send(()).unwrap();
    futures::future::join_all(tasks).await;

    let total_reqs = *total_requests.lock().await;
    let throughput = total_reqs as f64 / opts.duration_sec as f64;

    let hist = histogram.lock().await;
    println!("\n--- Benchmark Summary ---");
    println!("Total Requests: {}", total_reqs);
    println!("Throughput: {:.2} req/s", throughput);
    println!("\n--- Latency Distribution ---");
    println!(
        "p50: {} µs, p90: {} µs, p99: {} µs, p99.9: {} µs",
        hist.value_at_percentile(50.0),
        hist.value_at_percentile(90.0),
        hist.value_at_percentile(99.0),
        hist.value_at_percentile(99.9)
    );
    println!("--------------------------\n");

    Ok(())
}
