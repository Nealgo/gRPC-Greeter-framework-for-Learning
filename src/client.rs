use std::fs::File;
use std::io::{BufWriter, Write};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tonic::transport::Endpoint; // 引入 Endpoint

use hello_world::greeter_client::GreeterClient;
use hello_world::HelloRequest;

pub mod hello_world {
    tonic::include_proto!("helloworld");
}

#[derive(Debug)]
struct TaskResult {
    latency: Duration,
    is_success: bool,
}

#[derive(Debug)]
struct BenchmarkResult {
    concurrency: u32,
    rps: f64,
    avg_latency: Duration,
    p50_latency: Duration,
    p95_latency: Duration,
    p99_latency: Duration,
}

async fn run_concurrency_benchmark(
    server_addr: &str,
    concurrency: u32,
    total_requests: u32,
) -> Result<BenchmarkResult, Box<dyn std::error::Error>> {
    let (tx, mut rx) = mpsc::channel::<TaskResult>(50000); // 稍微加大一点 channel 缓冲

    let benchmark_start = Instant::now();

    // 1. 创建连接池
    let pool_size = 100; 
    let mut channels = Vec::with_capacity(pool_size);
    
    // 建立连接不计入 RPS 计算时间，或者你可以选择计入，
    // 但对于高吞吐测试，我们通常更关心连接建立后的“稳定吞吐”。
    // 这里我们先建好连接。
    for _ in 0..pool_size {
        let channel = Endpoint::from_shared(server_addr.to_string())?
            .tcp_nodelay(true) // 【建议】开启 TCP_NODELAY，减少延迟抖动
            .connect()
            .await?;
        channels.push(channel);
    }

    // 2. 启动压测任务
    // 记录开始时间（排除连接建立时间，测纯粹的 Request/Response 处理能力）
    let processing_start = Instant::now();

    for i in 0..concurrency {
        let tx_clone = tx.clone();
        let channel = channels[i as usize % pool_size].clone();
        let mut client = GreeterClient::new(channel);

        let requests_per_task = total_requests / concurrency;

        tokio::spawn(async move {
            for _ in 0..requests_per_task {
                let request = tonic::Request::new(HelloRequest { name: "Tonic".into() });
                let task_start = Instant::now();
                let response = client.say_hello(request).await;
                let latency = task_start.elapsed();
                
                let result = TaskResult { latency, is_success: response.is_ok() };
                
                // 忽略错误，防止 channel 满导致 panic，实际压测中 channel 可能会满
                let _ = tx_clone.send(result).await; 
            }
        });
    }

    // 关闭发送端，等待接收完成
    drop(tx);

    let mut results = Vec::with_capacity(total_requests as usize);
    while let Some(result) = rx.recv().await {
        results.push(result);
    }

    // 计算耗时（使用 processing_start）
    let benchmark_duration = processing_start.elapsed();
    let total_processed = results.len();

    if total_processed == 0 {
        return Err("No requests were processed.".into());
    }

    let mut latencies: Vec<Duration> = results
        .iter()
        .filter_map(|r| if r.is_success { Some(r.latency) } else { None })
        .collect();
    latencies.sort();

    let rps = total_processed as f64 / benchmark_duration.as_secs_f64();
    
    // 简单的统计逻辑
    let avg_latency = if !latencies.is_empty() {
        latencies.iter().sum::<Duration>() / latencies.len() as u32
    } else {
        Duration::default()
    };
    let p50 = latencies.get(latencies.len() / 2).cloned().unwrap_or_default();
    let p95 = latencies.get((latencies.len() as f64 * 0.95) as usize).cloned().unwrap_or_default();
    let p99 = latencies.get((latencies.len() as f64 * 0.99) as usize).cloned().unwrap_or_default();

    Ok(BenchmarkResult {
        concurrency,
        rps,
        avg_latency,
        p50_latency: p50,
        p95_latency: p95,
        p99_latency: p99,
    })
}

#[tokio::main]  
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server_addr = "http://[::1]:50051";
    
    // 【关键修改 1】加大请求量！
    // 之前是 100,000。对于 20万 RPS 的目标，这只够跑 0.5秒。
    // 改为 5,000,000 (500万)，确保每个测试跑 20秒左右，给 CPU 足够的时间满载。
    let total_requests_per_run = 5_000_000; 

    let start_concurrency = 10;
    let max_concurrency = 400;
    let stride = 20; // 稍微调大步长，因为测试时间变长了，减少等待时间

    let concurrency_levels: Vec<u32> = (start_concurrency..=max_concurrency)
        .step_by(stride)
        .collect();

    let output_filename = "benchmark_results.csv";
    let file = File::create(output_filename)?;
    let mut writer = BufWriter::new(file);
    writeln!(writer, "concurrency,rps,avg_latency_ms,p50_latency_ms,p95_latency_ms,p99_latency_ms")?;

    println!("Starting benchmark suite (Dual Core)...");
    println!("Requests per run: {}", total_requests_per_run);

    for &concurrency in concurrency_levels.iter() {
        println!("Running test with concurrency = {}...", concurrency);

        let result = run_concurrency_benchmark(server_addr, concurrency, total_requests_per_run).await?;

        println!(
            " -> Concurrency: {}, RPS: {:.2}, P99: {:.2}ms", 
            result.concurrency, result.rps, result.p99_latency.as_secs_f64() * 1000.0
        );

        writeln!(
            writer,
            "{},{},{:.4},{:.4},{:.4},{:.4}",
            result.concurrency, result.rps, 
            result.avg_latency.as_secs_f64() * 1000.0,
            result.p50_latency.as_secs_f64() * 1000.0,
            result.p95_latency.as_secs_f64() * 1000.0,
            result.p99_latency.as_secs_f64() * 1000.0
        )?;

        // 【关键修改 2】冷却时间
        // 每次测试跑完后，休息 2 秒。
        // 让操作系统处理完上一个测试遗留的 TIME_WAIT 连接，
        // 防止下一个测试因为端口耗尽而抖动。
        tokio::time::sleep(Duration::from_secs(2)).await; 
    }

    println!("\nBenchmark suite finished successfully!");
    Ok(())
}