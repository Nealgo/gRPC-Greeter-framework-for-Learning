use hello_world::greeter_client::GreeterClient;
use hello_world::HelloRequest;
use std::fs::File;
use std::io::Write;
use std::time::{Duration, Instant};
use tokio_stream::StreamExt;
use tonic::transport::Channel;

pub mod hello_world {
    tonic::include_proto!("helloworld");
}

#[derive(Debug, Clone)]
struct BenchResult {
    payload_size: usize,
    config_desc: String,
    total_duration: Duration,
    total_messages: usize,
    total_bytes: usize,
    
    // --- 改动：区分两个指标 ---
    ttfb: Duration,           // 真正的 TTFB (收到 HTTP Headers 的时间)
    first_msg_latency: Duration, // 首消息延迟 (收到完整第一个包的时间)
    
    throughput_qps: f64,
    throughput_mbps: f64,
    avg_latency: Duration,
    jitter: Duration,
    max_latency: Duration,
}

async fn run_stream_benchmark(
    client: &mut GreeterClient<Channel>,
    msg_count: usize,
    payload_size: usize,
) -> Result<BenchResult, Box<dyn std::error::Error>> {
    let param = format!("{}:{}", msg_count, payload_size);
    let request = tonic::Request::new(HelloRequest {
        name: param.clone(),
    });

    let start_req_time = Instant::now();

    // 1. 测量真正的 TTFB (Time To Headers)
    // 当这个 await 返回时，表示连接建立，Headers 已接收，但 Body 还没读
    let response = client.say_hello_stream(request).await?;
    let ttfb = start_req_time.elapsed();

    let mut stream = response.into_inner();

    // 2. 测量首消息延迟 (Time To First Message Body)
    // 这里等待第一个完整的数据包传输完成
    let first_msg = stream.next().await;
    let first_msg_latency = start_req_time.elapsed();

    if first_msg.is_none() {
        return Err("Stream returned no data!".into());
    }

    let mut inter_msg_latencies: Vec<Duration> = Vec::with_capacity(msg_count);
    let mut last_msg_time = Instant::now(); 
    let mut total_bytes = 0;
    let mut received_count = 1;

    if let Some(Ok(ref msg)) = first_msg {
        total_bytes += msg.message.len();
    }

    while let Some(response) = stream.next().await {
        let now = Instant::now();
        let latency = now.duration_since(last_msg_time);
        inter_msg_latencies.push(latency);
        last_msg_time = now;

        match response {
            Ok(reply) => {
                received_count += 1;
                total_bytes += reply.message.len();
            }
            Err(e) => eprintln!("Stream Error: {}", e),
        }
    }
    /*while let await 循环代码解读：
    等待 server push 或下一条数据产生，如果收到一条数据 → 进入循环，如果流结束（比如 server close stream）→ 退出循环。*/

    let total_duration = start_req_time.elapsed();

    let throughput_qps = received_count as f64 / total_duration.as_secs_f64();
    let throughput_mbps = (total_bytes as f64 / 1024.0 / 1024.0) / total_duration.as_secs_f64();

    let (avg_latency, jitter, max_latency) = if !inter_msg_latencies.is_empty() {
        let sum_micros: u128 = inter_msg_latencies.iter().map(|d| d.as_micros()).sum();
        let count = inter_msg_latencies.len() as u128;
        let mean_micros = sum_micros as f64 / count as f64;

        let variance: f64 = inter_msg_latencies.iter()
            .map(|d| {
                let diff = d.as_micros() as f64 - mean_micros;
                diff * diff
            })
            .sum::<f64>() / count as f64;
        
        let std_dev_micros = variance.sqrt();
        let max = inter_msg_latencies.iter().max().cloned().unwrap_or(Duration::ZERO);

        (
            Duration::from_micros(mean_micros as u64),
            Duration::from_micros(std_dev_micros as u64),
            max
        )
    } else {
        (Duration::ZERO, Duration::ZERO, Duration::ZERO)
    };

    Ok(BenchResult {
        payload_size,
        config_desc: format!("{} msgs * {} bytes", msg_count, payload_size),
        total_duration,
        total_messages: received_count,
        total_bytes,
        ttfb,               // Header 时间
        first_msg_latency,  // Body 时间
        throughput_qps,
        throughput_mbps,
        avg_latency,
        jitter,
        max_latency,
    })
}

fn write_csv_header(file: &mut File) -> std::io::Result<()> {
    // 增加了一列 First_Msg_Micros
    writeln!(file, "Payload_Bytes,Msg_Count,Total_Time_Sec,TTFB_Micros,First_Msg_Micros,QPS,MBPS,Avg_Lat_Micros,Jitter_Micros,Max_Lat_Micros")
}

fn write_csv_row(file: &mut File, res: &BenchResult) -> std::io::Result<()> {
    writeln!(
        file,
        "{},{},{:.6},{},{},{:.2},{:.2},{:.2},{:.2},{:.2}",
        res.payload_size,
        res.total_messages,
        res.total_duration.as_secs_f64(),
        res.ttfb.as_micros(),             // 真正的 TTFB
        res.first_msg_latency.as_micros(),// 首消息延迟
        res.throughput_qps,
        res.throughput_mbps,
        res.avg_latency.as_micros(),
        res.jitter.as_micros(),
        res.max_latency.as_micros()
    )
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let max_msg_size = 64 * 1024 * 1024;
    let mut client = GreeterClient::connect("http://[::1]:50051")
        .await?
        .max_decoding_message_size(max_msg_size)
        .max_encoding_message_size(max_msg_size);

    println!("Connected to server. Warming up...");

    // 预热
    let _ = run_stream_benchmark(&mut client, 100, 1024).await?;
    println!("Warm-up finished. Starting benchmark...\n");

    let file_path = "grpc_benchmark_results.csv";
    let mut file = File::create(file_path)?;
    write_csv_header(&mut file)?;

    let payload_sizes = [
        vec![32, 64, 128, 256, 512, 768, 1024, 1536, 2048, 3072],
        vec![4096, 8192, 12288, 16384, 24576, 32768, 40960, 49152, 57344, 65536],
        vec![
            131072, 262144, 393216, 524288, 786432, 
            1048576, 1572864, 2097152, 3145728, 4194304
        ]
    ].concat();

    let rounds = 3; 

    for &size in &payload_sizes {
        let count = if size <= 3072 { 20_000 } else if size <= 65536 { 5_000 } else { 1_000 };
        
        let mut round_results = Vec::with_capacity(rounds);

        print!("Benchmarking {} bytes ({} rounds): ", size, rounds);
        for _ in 0..rounds {
            match run_stream_benchmark(&mut client, count, size).await {
                Ok(res) => {
                    print!("."); 
                    std::io::stdout().flush()?;
                    round_results.push(res);
                }
                Err(_e) => eprintln!("X"),
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        println!(); 

        if !round_results.is_empty() {
            round_results.sort_by(|a, b| a.throughput_mbps.partial_cmp(&b.throughput_mbps).unwrap());
            let median_result = &round_results[round_results.len() / 2];
            write_csv_row(&mut file, median_result)?;
            
            println!("  -> Median: {:.2} MB/s | TTFB: {}us | 1st Msg: {}us", 
                median_result.throughput_mbps, 
                median_result.ttfb.as_micros(),
                median_result.first_msg_latency.as_micros());
        }
    }

    println!("\nAll benchmarks finished. Results saved to {}", file_path);
    Ok(())
}