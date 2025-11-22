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

// 1. 定义测试结果结构体
#[derive(Debug)]
struct BenchResult {
    payload_size: usize,      // 新增：记录当前包大小
    config_desc: String,
    total_duration: Duration,
    total_messages: usize,
    total_bytes: usize,
    ttfb: Duration,           // 指标 A
    throughput_qps: f64,      // 指标 B1
    throughput_mbps: f64,     // 指标 B2
    avg_latency: Duration,    // 指标 C
    jitter: Duration,         // 指标 D (标准差)
    max_latency: Duration,    // 最大抖动
}

// 2. 独立的 benchmark 函数
async fn run_stream_benchmark(
    client: &mut GreeterClient<Channel>,
    msg_count: usize,
    payload_size: usize,
) -> Result<BenchResult, Box<dyn std::error::Error>> {
    let param = format!("{}:{}", msg_count, payload_size);
    let request = tonic::Request::new(HelloRequest {
        name: param.clone(),
    });

    println!("--> Running benchmark: {} msgs, {} bytes payload...", msg_count, payload_size);

    let start_req_time = Instant::now();

    let mut stream = client.say_hello_stream(request).await?.into_inner();

    // 测量 TTFB
    let ttfb_start = Instant::now();
    let first_msg = stream.next().await;
    let ttfb = ttfb_start.elapsed();

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

    let total_duration = start_req_time.elapsed();

    // 计算统计指标
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
        payload_size, // 记录大小
        config_desc: format!("{} msgs * {} bytes", msg_count, payload_size),
        total_duration,
        total_messages: received_count,
        total_bytes,
        ttfb,
        throughput_qps,
        throughput_mbps,
        avg_latency,
        jitter,
        max_latency,
    })
}

// 辅助函数：打印控制台报告
fn print_report(res: &BenchResult) {
    println!("Summary for [{}]:", res.config_desc);
    println!("  Throughput: {:.2} MB/s ({:.2} QPS)", res.throughput_mbps, res.throughput_qps);
    println!("  Latency:    Avg {:.2} us | Jitter {:.2} us", res.avg_latency.as_micros() as f64, res.jitter.as_micros() as f64);
    println!("----------------------------------------");
}

// 辅助函数：将结果写入 CSV
fn write_csv_header(file: &mut File) -> std::io::Result<()> {
    // 写入 CSV 表头
    writeln!(file, "Payload_Bytes,Msg_Count,Total_Time_Sec,TTFB_Micros,QPS,MBPS,Avg_Lat_Micros,Jitter_Micros,Max_Lat_Micros")
}

fn write_csv_row(file: &mut File, res: &BenchResult) -> std::io::Result<()> {
    // 将 Duration 转换为微秒(us)或秒(s)的纯数字，方便绘图
    writeln!(
        file,
        "{},{},{:.6},{},{:.2},{:.2},{:.2},{:.2},{:.2}",
        res.payload_size,
        res.total_messages,
        res.total_duration.as_secs_f64(),
        res.ttfb.as_micros(),
        res.throughput_qps,
        res.throughput_mbps,
        res.avg_latency.as_micros(), // 使用微秒更精确
        res.jitter.as_micros(),
        res.max_latency.as_micros()
    )
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 设置最大消息大小为 64MB
    let max_msg_size = 64 * 1024 * 1024;

    // 关键修改：在 connect 时配置 limit
    let mut client = GreeterClient::connect("http://[::1]:50051")
        .await?
        .max_decoding_message_size(max_msg_size)
        .max_encoding_message_size(max_msg_size);

    println!("Connected to server (Max msg size: 64MB).\n");

    // 2. 创建 CSV 文件
    let file_path = "grpc_benchmark_results.csv";
    let mut file = File::create(file_path)?;
    write_csv_header(&mut file)?;
    println!("Created output file: {}", file_path);

    // 3. 定义精细化的测试场景：小、中、大各 10 个采样点
    // 使用 Vec::concat 将它们合并为一个测试列表
    let payload_sizes = [
        // --- 小包 (Small): 32B ~ 3KB ---
        // 侧重测试 PPS (包/秒) 和 Latency (延迟)
        // 这里的瓶颈通常是 CPU (序列化/反序列化) 和 系统调用
        vec![32, 64, 128, 256, 512, 768, 1024, 1536, 2048, 3072],

        // --- 中包 (Medium): 4KB ~ 64KB ---
        // 侧重测试内存拷贝效率和 TCP 窗口效应
        // 4KB 是常见的内存页大小，64KB 是常见的 TCP 窗口或 I/O buffer 大小
        vec![4096, 8192, 12288, 16384, 24576, 32768, 40960, 49152, 57344, 65536],

        // --- 大包 (Large): 128KB ~ 4MB ---
        // 侧重测试吞吐量 (Bandwidth) 和 抖动 (Jitter)
        // 注意：gRPC 默认限制通常在 4MB 左右，如果超过 4MB 可能需要调整 Server/Client 配置
        vec![
            131072,  // 128KB
            262144,  // 256KB
            393216,  // 384KB
            524288,  // 512KB
            786432,  // 768KB
            1048576, // 1MB
            1572864, // 1.5MB
            2097152, // 2MB
            3145728, // 3MB
            4194304  // 4MB (注意：如果报错 ResourceExhausted，请减小这个值)
        ]
    ].concat();

    // 预估总耗时提示
    println!("Total test cases: {}", payload_sizes.len());
    println!("Estimated run time: ~2-3 minutes...\n");

    for &size in &payload_sizes {
        // 动态调整发送数量逻辑优化
        let count = if size <= 3072 {
            // 小包：必须发大量数据才能平摊系统调用开销，测出真实的微秒级延迟
            20_000 
        } else if size <= 65536 {
            // 中包：适当减少，保证测试速度
            5_000  
        } else { // 1MB 以下的大包
            // 1MB * 1000 = 1GB 数据量，耗时约 1-2秒
            1_000  
        };

        // 运行测试
        match run_stream_benchmark(&mut client, count, size).await {
            Ok(result) => {
                print_report(&result);
                write_csv_row(&mut file, &result)?;
            }
            Err(e) => {
                eprintln!("Failed to run benchmark for size {}: {}", size, e);
                // 如果在大包测试时失败（例如超过 gRPC 限制），不要中断整个循环，继续下一个
            }
        }
        
        // 稍微延时，让服务端的 TCP 缓冲区和内存回收喘口气
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // ... 后面的代码保持不变 ...

    println!("\nAll benchmarks finished. Results saved to {}", file_path);
    Ok(())
}