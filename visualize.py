import pandas as pd
import matplotlib.pyplot as plt
import sys

# 定义输入文件名，与 Rust 程序中定义的一致
INPUT_FILENAME = "benchmark_results.csv"

def visualize_results():
    """
    从 CSV 文件读取数据并生成图表。
    """
    print(f"--- Reading benchmark data from '{INPUT_FILENAME}' ---")
    
    try:
        # 使用 Pandas 读取 CSV 文件
        df = pd.read_csv(INPUT_FILENAME)
    except FileNotFoundError:
        print(f"Error: The file '{INPUT_FILENAME}' was not found.")
        print("Please run the Rust benchmark client first to generate the results file.")
        sys.exit(1) # 退出脚本

    print("--- Benchmark Data ---")
    print(df)

    # --- 创建可视化图表 ---
    print("\n--- Generating Visualization ---")
    fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(10, 12))
    fig.suptitle('gRPC Server Performance Benchmark', fontsize=16)

    # --- 子图 1: 吞吐量 (Throughput) vs. 并发数 (Concurrency) ---
    # (这部分保持不变)
    ax1.plot(df['concurrency'], df['rps'], marker='o', linestyle='-', color='b')
    ax1.set_title('Throughput vs. Concurrency')
    ax1.set_xlabel('Concurrency Level')
    ax1.set_ylabel('Requests Per Second (RPS)')
    ax1.grid(True)

    # --- 子图 2: 延迟百分位 (Latency Percentiles) vs. 并发数 (Concurrency) ---
    # ****** 这是修改的核心部分 ******
    # 我们将在这里同时绘制 P50, P95, 和 P99 延迟
    
    # P50 (Median) - 代表普通用户体验
    ax2.plot(df['concurrency'], df['p50_latency_ms'], marker='s', linestyle='-', color='g', label='p50 (Median) Latency')
    
    # P95 - 代表较差的用户体验
    # 注意：您的 Rust 脚本生成的是 p95，我们这里就绘制 p95。
    ax2.plot(df['concurrency'], df['p95_latency_ms'], marker='^', linestyle='--', color='orange', label='p95 Latency')
    
    # P99 - 代表最差的用户体验 (长尾延迟)
    ax2.plot(df['concurrency'], df['p99_latency_ms'], marker='x', linestyle=':', color='r', label='p99 Latency')

    ax2.set_title('Latency Percentiles vs. Concurrency')
    ax2.set_xlabel('Concurrency Level')
    ax2.set_ylabel('Latency (ms)')
    ax2.legend() # 显示图例，区分三条线
    ax2.grid(True)

    plt.tight_layout(rect=[0, 0.03, 1, 0.95])
    
    # 将图表保存到文件
    output_plot_filename = "benchmark_plot.png"
    fig.savefig(output_plot_filename)
    print(f"Plot saved to '{output_plot_filename}'")
    
    # 显示图表
    plt.show()


if __name__ == "__main__":
    visualize_results()