### 实验背景
分布式文件系统中，**Stream RPC** 扮演着不可替代的角色，主要用于以下场景：

+ **大规模元数据扫描 (Metadata Scanning)**
    - 场景: 用户执行 ls -R (递归列出目录) 或数据库进行 Table Scan。
    - 原因: 如果一个目录有 100 万个文件，使用 Unary RPC（一问一答）需要 100 万次 RTT（往返时延），效率极低。使用 Server Streaming，客户端发一个请求，服务端源源不断地吐出 DirEntry，几乎填满带宽，能够极大降低延迟。
    - LeoFS 映射: 你的底层 TiKV 在进行 Coprocessor 范围查询时，本质上也是流式返回数据。
+ **Watch 机制 / 事件通知**
    - 场景: Kubernetes 监听配置变化，或者客户端通过 inotify 监听文件改动。
    - 原因: 这是一个长连接流。客户端发起 Watch(path)，连接保持打开，服务端一有变更就推送事件。这是典型的 Server Streaming。
+ **日志复制与快照传输 (Log Replication & Snapshots)**
    - 场景: Raft 共识协议中的日志同步，或者节点加入时的全量快照传输。
    - 原因: 快照通常很大（GB级别），必须通过流式传输分块发送，防止内存溢出并支持断点续传。
+ **大文件读写 (Client/Bi-directional Streaming)**
    - 场景: 客户端写入一个 1GB 的文件。
    - 原因: 客户端不会一次性把 1GB 塞进内存，而是通过 Client Streaming 不断发送 64KB/4MB 的 Chunk，服务端随收随写。虽然 LeoFS 数据层用了 RDMA，但如果客户端不支持 RDMA（如普通 TCP 客户端），这层 gRPC Stream 就是数据通道的命脉。

### 测量指标&原因
一共选择四个指标进行测试：**首字节延迟、吞吐量、消息间延迟、抖动**。

#### TTFB： Time To First Byte (首字节延迟):
在 DFS 中对应 "Latency to first read"。用户不在乎你吞吐量有 100GB/s，只在乎打开视频时是不是立即开始播放。TTFB 直接衡量了元数据寻址和磁盘 Seek 的耗时。

#### 吞吐量&QPS
吞吐量是衡量在单位时间内传输了多少数据或消息。

+ **QPS (Queries Per Second)：** 每秒接收的消息数。
+ **MB/s (Megabytes Per Second)：** 每秒接收的数据量（兆字节）。

**注意：** 这里的 QPS 计算基于总时长，反映了整个流式过程的 **平均效率**。

#### Inter-Message Latency (消息间延迟)
由谁决定: 这反映了服务端处理逻辑的稳定性。在 Rust 中，如果你的 Tokio 线程因为 CPU 密集型任务（如 Protobuf 序列化大对象）阻塞了，或者 TiKV 发生了 RocksDB Compaction 导致的抖动，这个指标会瞬间飙升。

#### Jitter (抖动)
HPC 痛点: 在高性能计算（AI 训练）中，所有 GPU 需要同步 barrier。如果数据流抖动大，快的 GPU 就要等慢的，导致整个集群算力浪费。证明 LeoFS 的低抖动是极大的亮点。

### 实现方式
#### 首字节延迟
```rust
let ttfb_start = Instant::now();
let first_msg = stream.next().await;
let ttfb = ttfb_start.elapsed();
```

计算的是第一个

#### 吞吐量&QPS
$ \text{QPS} = \frac{\text{接收到的消息总数}}{\text{总耗时（秒）}} $

$ \text{MB/s} = \frac{\text{传输的总字节数} / (1024 \times 1024)}{\text{总耗时（秒）}} $

#### 消息间延迟
这是衡量数据流的 **平滑性** 和 **服务器出包速率** 的关键指标。

```rust
let now = Instant::now();
let latency = now.duration_since(last_msg_time); // 计算与上一条消息之间的时间差
inter_msg_latencies.push(latency); // 存储每个时间差
last_msg_time = now;
```

##### 消息间最大延迟
$ Interval_1 = Time(Msg_2) - Time(Msg_1)
 $

$ undefined $

$ ...
 $

$ Interval_N = Time(Msg_{N+1}) - Time(Msg_N) $

然后选择其中$ Interval_i $中最大的一个作为消息间最大延迟。

##### 消息间平均延迟
> 显而易见，求平均即可。
>

##### 抖动
> 计算消息延迟之间的标准差。
>

抖动衡量了消息间延迟的 **稳定性**，即数据包到达时间是否一致。

+ **<font style="color:#DF2A3F;">计算标准差 (Jitter):</font>**<font style="color:#DF2A3F;"> 代码使用了标准的统计学方法计算 </font>**<font style="color:#DF2A3F;">标准差 (Standard Deviation)</font>**：
    1. 计算所有消息间延迟与平均延迟之间的 **差值**。
    2. 计算这些差值的 **平方** (variance / 方差)。
    3. 计算方差的 **平方根** (std_dev / 标准差)。

```rust
// ... 中间变量略 ...
let variance: f64 = inter_msg_latencies.iter()
    .map(|d| {
        let diff = d.as_micros() as f64 - mean_micros;
        diff * diff
    })
    .sum::<f64>() / count as f64;

let std_dev_micros = variance.sqrt(); // Jitter
```

### 实验结果
#### 第一次实验数据分析
| <font style="color:black;">Payload_Bytes</font> | <font style="color:black;">Msg_Count</font> | <font style="color:black;">Total_Time_Sec</font> | <font style="color:black;">TTFB_Micros</font> | <font style="color:black;">QPS</font> | <font style="color:black;">MBPS</font> | <font style="color:black;">Avg_Lat_Micros</font> | <font style="color:black;">Jitter_Micros</font> | <font style="color:black;">Max_Lat_Micros</font> |
| :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| <font style="color:black;">128</font> | <font style="color:black;">20000</font> | <font style="color:black;">0.044594</font> | <font style="color:black;">416</font> | <font style="color:black;">448487.5</font> | <font style="color:black;">59.21</font> | <font style="color:black;">2</font> | <font style="color:black;">25</font> | <font style="color:black;">559</font> |
| <font style="color:black;">512</font> | <font style="color:black;">20000</font> | <font style="color:black;">0.048555</font> | <font style="color:black;">403</font> | <font style="color:black;">411906</font> | <font style="color:black;">205.23</font> | <font style="color:black;">1</font> | <font style="color:black;">21</font> | <font style="color:black;">1489</font> |
| <font style="color:black;">1024</font> | <font style="color:black;">20000</font> | <font style="color:black;">0.075794</font> | <font style="color:black;">316</font> | <font style="color:black;">263872.3</font> | <font style="color:black;">260.32</font> | <font style="color:black;">3</font> | <font style="color:black;">23</font> | <font style="color:black;">1174</font> |
| <font style="color:black;">4096</font> | <font style="color:black;">5000</font> | <font style="color:black;">0.055196</font> | <font style="color:black;">1738</font> | <font style="color:black;">90585.97</font> | <font style="color:black;">354.7</font> | <font style="color:black;">10</font> | <font style="color:black;">84</font> | <font style="color:black;">5223</font> |
| <font style="color:black;">16384</font> | <font style="color:black;">5000</font> | <font style="color:black;">0.174198</font> | <font style="color:black;">693</font> | <font style="color:black;">28702.91</font> | <font style="color:black;">448.75</font> | <font style="color:black;">34</font> | <font style="color:black;">62</font> | <font style="color:black;">1053</font> |
| <font style="color:black;">65536</font> | <font style="color:black;">1000</font> | <font style="color:black;">0.123159</font> | <font style="color:black;">2161</font> | <font style="color:black;">8119.59</font> | <font style="color:black;">507.54</font> | <font style="color:black;">120</font> | <font style="color:black;">136</font> | <font style="color:black;">3220</font> |
| <font style="color:black;">262144</font> | <font style="color:black;">1000</font> | <font style="color:black;">0.492621</font> | <font style="color:black;">44551</font> | <font style="color:black;">2029.96</font> | <font style="color:black;">507.51</font> | <font style="color:black;">447</font> | <font style="color:black;">911</font> | <font style="color:black;">28937</font> |
| <font style="color:black;">524288</font> | <font style="color:black;">1000</font> | <font style="color:black;">0.827777</font> | <font style="color:black;">68347</font> | <font style="color:black;">1208.05</font> | <font style="color:black;">604.04</font> | <font style="color:black;">758</font> | <font style="color:black;">1289</font> | <font style="color:black;">41375</font> |
| <font style="color:black;">1048576</font> | <font style="color:black;">1000</font> | <font style="color:black;">1.817468</font> | <font style="color:black;">89514</font> | <font style="color:black;">550.22</font> | <font style="color:black;">550.22</font> | <font style="color:black;">1727</font> | <font style="color:black;">2684</font> | <font style="color:black;">85762</font> |


<font style="color:black;">上述实验数据中，未采用控制变量法的原因如下：</font>

+ 小包 (128B - 1KB): 测的是 CPU 和 PPS (Packets Per Second)
    - 处理一个小包极快（你的数据里只需 2微秒）。
    - 如果是 1000 次循环，总耗时可能只有几毫秒。在这个尺度下，操作系统调度抖动（OS Noise）、CPU 变频等“噪音”占比极大。
    - 必须用巨大的 msg_count (20,000+) 来拉长测试时间，通过“大数定律”平滑掉噪音，得到准确的平均值。
+ 大包 (64KB - 1MB): 测的是 带宽 和 内存拷贝 (Bandwidth & Memcpy)
    - 处理一个大包很慢（你的数据里需要 1727微秒）。
    - 单次操作已经足够“重”，噪音影响较小。
    - 1000 次采样对于统计学上的正态分布分析（求均值、方差）已经完全足够（通常 N > 30 就有统计意义）。

#### 第二次实验数据分析
**图表规划策略**

1. **图 1：吞吐量瓶颈分析 (The Throughput Wall)**
+ 目的：展示 gRPC 在基于内核 TCP 协议栈时，无论 Payload 多大，带宽都卡在 ~600MB/s (约 5Gbps) 左右。
+ X轴：Payload Size (对数坐标 Log Scale，因为横跨 32B 到 4MB)。
+ Y轴：Throughput (MB/s)。

核心结论：证明软件和协议栈是瓶颈，无法跑满 200Gbps RDMA 硬件，论证 LeoFS 引入 RDMA/Bypass Kernel 的必要性。

2. **图 2：延迟与抖动分析 (Latency & Jitter Stability)**
+ 目的：展示在大包场景下，抖动（Jitter）如何恶化。
+ X轴：Payload Size (Log Scale)。
+ Y轴：Time (µs) (Log Scale)。

内容：画两条线，一条是 Avg_Lat，一条是 Jitter。

核心结论：在 4MB 时，抖动（~7ms）甚至超过了平均延迟（~6ms）。论证 gRPC 不适合对稳定性要求极高的 HPC/AI 训练场景。

3. **图 3：QPS 衰减分析 (System Overhead)**
+ 目的：展示小 I/O 的高并发能力 vs 大 I/O 的拷贝开销。
+ X轴：Payload Size (Log Scale)。
+ Y轴：QPS (Log Scale)。

核心结论：QPS 呈幂律下降，说明小包受限于 CPU/PPS，大包受限于带宽/拷贝。

**图如下**

> 暂时略去。
>

#### 实验方案改进
**解决步骤1：**

+ CPU 抢占 (Context Switching)：如果别人的程序突然占用 CPU，操作系统会暂停你的程序，导致你的 Latency 出现巨大的抖动（Jitter）。
+ 缓存污染 (Cache Pollution)：别人的程序会把 CPU L1/L2/L3 缓存里的数据冲掉，导致你的代码必须去读内存，吞吐量骤降。
+ 内存带宽争抢：如果别人在疯狂进行 memcpy，你的内存读写速度会变慢。

```rust
cargo build --release --bin server
cargo build --release --bin client


taskset -c 10 ./target/release/server
taskset -c 11 ./target/release/client
```

**解决步骤2：**

+ 预热 (Warm-up)：正式测试前先跑一跑，把连接建好，缓存填满。

> main 函数开头增加了一次跑 100 条消息的测试。这确保了后续正式测试时，TCP 连接已经是热的，且 Rust 的运行时（Tokio Runtime）已经分配好了线程池。
>

+ 多轮择优 (Best of N / Median)：针对每个 Payload 大小，跑 3~5 次，去掉最差的（被干扰的），取中位数或最大吞吐量。

### 实验结论分析
#### 吞吐量
`Payload/AvgLatency≈Throughput`

![](https://cdn.nlark.com/yuque/0/2025/png/54083526/1763810997208-4cbd8601-2c4d-4dfe-aed8-9aa6ad3d6513.png)

“峰值性能（Peak Performance）” 与 “大块 I/O 稳定性能（Sustained Large I/O Performance）” 是两个完全不同的概念。

峰值出现在 <font style="color:#DF2A3F;">16KB - 64KB</font> 这个区间，现代 CPU 的 L2 缓存通常在 256KB 到 1MB 之间。当你发送 64KB 的数据时，这块数据完全装在 CPU 的 L2 甚至 L1 缓存里。看横坐标的最右侧，4MB 的位置，掉到了 500 MB/s。4MB 的数据太大了，L2 缓存根本装不下，甚至 L3 缓存都被挤爆了。CPU 必须去“主内存（DRAM）”里搬运数据。

`用户态对象 -> Protobuf 序列化 -> 内核 Socket Buffer -> 网卡 DMA`。这中间涉及多次内存拷贝。当数据量极大时，CPU 的主要时间都花在等待内存拷贝完成上。**具体的过程如下：**

:::tips
**[序列化]**

路径：Rust Struct (Heap) -> Serialized Buffer (User Space)  
原因：数据格式转换

Rust 代码中，数据可能分散在堆内存的各个角落。例如 HelloRequest 结构体，它的字段 name 可能指向堆的一个位置，data 指向另一个位置。这些数据在物理内存中是不连续的。然而，网络发送需要的是一段连续的、符合协议规范的二进制流。Protobuf 库（prost）必须开辟一块新的连续内存缓冲区，把 Tag、Length 写入头部，然后把你的 4MB 真实数据 memcpy 到这块缓冲区的后面。因此，CPU 必须遍历这 4MB 数据，把它搬运到新的位置。

**[系统调用]**

路径：Serialized Buffer (User Space) -> Socket Buffer (Kernel Space)  
原因：安全隔离 & TCP 协议特性

序列化好之后，Rust 调用 TcpStream::write()，底层触发 sendto 系统调用。操作系统不信任用户程序。如果内核直接使用你用户态的指针，万一你在网卡发送的过程中修改了这块内存怎么办？万一你把内存释放了怎么办？内核必须把数据拷贝到它自己管理的“安全区域”（Kernel Space）。TCP 是可靠传输协议。数据发出去后，不能立刻丢弃，必须保留副本。如果接收端没回复 ACK，发送端必须重传。用户态的程序发完数据可能就把 buffer 销毁了，所以内核必须在 Socket Buffer (SKB, Socket Buffer) 中保留一份数据副本，直到收到 ACK 为止。因此，CPU 触发上下文切换（Context Switch），进入内核态，执行 copy_from_user()，将 4MB 数据搬运到内核的 sk_buff 链表中。所以，会存在严重的 CPU 消耗 + CPU 缓存污染。4MB 的拷贝操作会把 L3 缓存里原有的有用数据全部冲掉，导致后续代码运行变慢。

:::

#### 平均延迟、抖动、首字节延迟、首消息延迟、长尾延迟
![](https://cdn.nlark.com/yuque/0/2025/png/54083526/1763889370546-4af7e2b4-61a9-4ea3-a11b-c8503838f9ad.png)

1. TTFB首字节延迟

基本上保持不变，这得益于gRPC优秀的底层框架，无论消息荷载多大，都基本维持在1ms左右。

2. Avg_Latency 平均延迟
3. Jitter 抖动

抖动和平均延迟在荷载较小的时候差距较大，这是因为小包的传输时间容易收到外界因素的干扰，然而，随着PayLoad的上升，两者的差距逐渐变小，个人猜测是因为操作系统的抖动对于大包传输的印象较小

4. Max_Latency 最大延迟
5. Fisrt_Msg_Latency 首消息延迟

接近4MB时：TCP 慢启动 (Slow Start)：连接刚建立时，TCP 拥塞窗口（CWND）极小，无法一次性发送 4MB 数据，必须经过多次 RTT 往返来“试探”网络带宽，显而易见。

#### QPS
![](https://cdn.nlark.com/yuque/0/2025/png/54083526/1763889416477-63f5015e-2ab3-4255-9416-c9d93a3fcf2f.png)

【区间：32B ~ 1KB】

+ 走势：曲线略微平缓，没有完全遵循严格的线性下降，QPS 维持在极高水平（约 70万）。
+ 物理含义：CPU 计算密集型瓶颈。
    - 在这个区间，包非常小，内存拷贝的开销可以忽略不计。
    - 主要耗时在于：系统调用（System Calls）、Tokio 事件循环调度、Protobuf 头部解析。
    - 这些开销是固定成本（Fixed Cost），不随 Payload 大小显著变化。
    - 评价：70万 QPS 证明了 Rust/Tokio 运行时极其高效，足以应对 LeoFS 的元数据（Metadata）高并发访问需求。

【区间：> 4KB】

+ 走势：标准的线性下跌。Payload 每增加 10 倍，QPS 就下降 10 倍（例如 10KB 时约 5万，100KB 时约 5千）。
+ 物理含义：内存拷贝密集型瓶颈。
    - 在这个区间，Payload 变大，数据拷贝（Memcpy）的时间成为主导，远远超过了系统调用的固定开销。
    - 处理一个请求的时间 $ T \approx \frac{Size}{Bandwidth} $
    - 因此，$ QPS = \frac{1}{T} \propto \frac{1}{Size} $
+ 评价：QPS 的迅速衰减说明 gRPC 在处理大块数据时效率极低。例如在 4MB 时，QPS 仅剩 ~130。这意味着如果在 LeoFS 的数据路径使用 gRPC，系统的 IOPS（每秒读写次数）将被彻底锁死。

