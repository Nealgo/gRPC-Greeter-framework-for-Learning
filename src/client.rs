use std::fs::File;
use std::io::{BufWriter, Write};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tonic::transport::{Channel, Endpoint};

use hello_world::greeter_client::GreeterClient;
use hello_world::HelloRequest;

pub mod hello_world {
    tonic::include_proto!("helloworld");
}

// 用于从并发任务收集结果
#[derive(Debug)]
struct TaskResult {
    latency: Duration,
    is_success: bool,
}

// 用于从 benchmark 函数返回结构化的结果
#[derive(Debug)]
struct BenchmarkResult {
    concurrency: u32,
    rps: f64,
    avg_latency: Duration,
    p50_latency: Duration,
    p95_latency: Duration,
    p99_latency: Duration,
}

// 修改后的函数，不再打印，而是返回 BenchmarkResult
async fn run_concurrency_benchmark(
    server_addr: &str,
    concurrency: u32,
    total_requests: u32,
) -> Result<BenchmarkResult, Box<dyn std::error::Error>> {
    let (tx, mut rx) = mpsc::channel::<TaskResult>(concurrency as usize);
    
    // 1.【删除】这行代码，我们不再需要这个单例 client
    // let client = GreeterClient::connect(server_addr.to_string()).await?; 

    let benchmark_start = Instant::now();

    // 2.【新增】创建连接池 (Connection Pool)
    let pool_size = 10; // 建议设置 10-20，足够喂饱单核 Server
    let mut channels = Vec::with_capacity(pool_size);
    
    // println!("Connecting to server with {} connections...", pool_size);
    for _ in 0..pool_size {
        // 使用 Endpoint::from_shared 处理动态地址字符串
        // connect().await 会建立真正的 TCP 连接
        let channel = Endpoint::from_shared(server_addr.to_string())?
            .connect()
            .await?;
        channels.push(channel);
    }

    // 3.【修改】并发循环
    for i in 0..concurrency {
        let tx_clone = tx.clone();

        // 4.【关键】轮询 (Round-Robin) 从池子中获取 Channel
        // 这样并发任务会均匀分布在这 10 条 TCP 连接上
        let channel = channels[i as usize % pool_size].clone();
        
        // 5.【关键】为当前任务创建一个专属的 Client
        let mut client = GreeterClient::new(channel);

        tokio::spawn(async move {
            let requests_per_task = total_requests / concurrency;
            for _ in 0..requests_per_task {
                let request = tonic::Request::new(HelloRequest { name: "Tonic".into() });
                let task_start = Instant::now();
                
                // 6.【关键】这里使用的是当前任务专属的 client，而不是外层的 clone
                let response = client.say_hello(request).await;
                
                let latency = task_start.elapsed();
                let result = TaskResult { latency, is_success: response.is_ok() };
                
                if tx_clone.send(result).await.is_err() {
                    // eprintln!("Receiver dropped, exiting task {}", i); // 压测时尽量减少 print
                    break;
                }
            }
        });
    }

    drop(tx);

    let mut results = Vec::with_capacity(total_requests as usize);
    while let Some(result) = rx.recv().await {
        results.push(result);
    }

    let benchmark_duration = benchmark_start.elapsed();
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
    let avg_latency = if !latencies.is_empty() {
        latencies.iter().sum::<Duration>() / latencies.len() as u32
    } else {
        Duration::default()
    };
    let p50_latency = latencies.get(latencies.len() / 2).cloned().unwrap_or_default();
    let p95_latency = latencies.get((latencies.len() as f64 * 0.95) as usize).cloned().unwrap_or_default();
    let p99_latency = latencies.get((latencies.len() as f64 * 0.99) as usize).cloned().unwrap_or_default();

    Ok(BenchmarkResult {
        concurrency,
        rps,
        avg_latency,
        p50_latency,
        p95_latency,
        p99_latency,
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // --- 1. 定义测试计划和输出文件 ---
    let server_addr = "http://[::1]:50051";
    let total_requests_per_run = 100000;

    

    let start_concurrency = 10;
    let max_concurrency = 400;
    let stride = 10; // The step or interval

    // 使用 Rust 的 range 和 iterators 自动生成测试列表
    let concurrency_levels: Vec<u32> = (start_concurrency..=max_concurrency)
        .step_by(stride)
        .collect();


    let output_filename = "benchmark_results.csv";

    // --- 2. 创建文件并写入 CSV 表头 ---
    let file = File::create(output_filename)?;
    let mut writer = BufWriter::new(file);
    writeln!(writer, "concurrency,rps,avg_latency_ms,p50_latency_ms,p95_latency_ms,p99_latency_ms")?;

    println!("Starting benchmark suite...");
    println!("Results will be saved to '{}'", output_filename);

    // --- 3. 循环执行所有测试 ---
    for &concurrency in concurrency_levels.iter() {
        println!("Running test with concurrency = {}...", concurrency);

        let result = run_concurrency_benchmark(server_addr, concurrency, total_requests_per_run).await?;

        // 将 Duration 转换为毫秒浮点数以便写入 CSV
        let avg_ms = result.avg_latency.as_secs_f64() * 1000.0;
        let p50_ms = result.p50_latency.as_secs_f64() * 1000.0;
        let p95_ms = result.p95_latency.as_secs_f64() * 1000.0;
        let p99_ms = result.p99_latency.as_secs_f64() * 1000.0;

        // --- 4. 将结果行写入文件 ---
        writeln!(
            writer,
            "{},{},{:.4},{:.4},{:.4},{:.4}",
            result.concurrency, result.rps, avg_ms, p50_ms, p95_ms, p99_ms
        )?;
    }

    println!("\nBenchmark suite finished successfully!");
    Ok(())
}