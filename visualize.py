import matplotlib.pyplot as plt
import pandas as pd
import matplotlib.ticker as ticker
import os
import sys

# =================配置区域=================
CSV_FILE_PATH = 'grpc_benchmark_results.csv'  # Rust 程序生成的 CSV 文件名
# =========================================

# 1. 检查文件是否存在
if not os.path.exists(CSV_FILE_PATH):
    print(f"错误: 找不到文件 '{CSV_FILE_PATH}'")
    print("请确保运行了 Rust 客户端基准测试，并且生成的 CSV 文件与此脚本在同一目录下。")
    sys.exit(1)

# 2. 读取 CSV 文件
try:
    df = pd.read_csv(CSV_FILE_PATH)
    print(f"成功读取数据: {len(df)} 行")
    print(df.head()) # 打印前几行确认数据正确
except Exception as e:
    print(f"读取 CSV 文件失败: {e}")
    sys.exit(1)

# 3. 设置绘图风格 (出版级风格)
plt.style.use('seaborn-v0_8-whitegrid')
plt.rcParams.update({
    'font.family': 'serif', # 衬线字体，适合论文
    'font.size': 12,
    'axes.labelsize': 14,
    'axes.titlesize': 16,
    'xtick.labelsize': 12,
    'ytick.labelsize': 12,
    'legend.fontsize': 12,
    'lines.linewidth': 2.5,
    'lines.markersize': 8,
    'figure.autolayout': True # 自动调整布局防止遮挡
})

# 辅助函数：将 X 轴数值格式化为 B, KB, MB
def format_xaxis(x, pos):
    if x < 1024: return f'{int(x)}B'
    elif x < 1024**2: return f'{int(x/1024)}KB'
    else: return f'{int(x/1024**2)}MB'

# ========================================================
# 图表 1：吞吐量瓶颈分析 (Throughput / Bandwidth)
# ========================================================
fig1, ax1 = plt.subplots(figsize=(10, 6))
ax1.plot(df['Payload_Bytes'], df['MBPS'], marker='o', color='#1f77b4', label='Throughput')

# 添加参考线：软件瓶颈 (取最大吞吐量作为参考)
max_throughput = df['MBPS'].max()
ax1.axhline(y=max_throughput, color='r', linestyle='--', alpha=0.5, label=f'Peak (~{int(max_throughput)} MB/s)')

ax1.set_xscale('log')
ax1.xaxis.set_major_formatter(ticker.FuncFormatter(format_xaxis))
ax1.set_xlabel('Payload Size (Log Scale)')
ax1.set_ylabel('Throughput (MB/s)')
ax1.set_title('gRPC Throughput Analysis: The Software Bottleneck')
ax1.legend()
ax1.grid(True, which="both", ls="-", alpha=0.2)

filename1 = 'chart1_throughput.png'
plt.savefig(filename1, dpi=300)
print(f"生成图片: {filename1}")
# plt.show() # 如果需要弹窗显示，取消注释

# ========================================================
# 图表 2：延迟与抖动 (Latency & Jitter)
# ========================================================
fig2, ax2 = plt.subplots(figsize=(10, 6))

# 使用对数坐标，因为从小包到大包延迟跨度极大
ax2.plot(df['Payload_Bytes'], df['Avg_Lat_Micros'], marker='s', color='#2ca02c', label='Avg Latency')
ax2.plot(df['Payload_Bytes'], df['Jitter_Micros'], marker='^', color='#d62728', linestyle='--', label='Jitter (Std Dev)')

ax2.set_xscale('log')
ax2.set_yscale('log') # Y轴也用对数
ax2.xaxis.set_major_formatter(ticker.FuncFormatter(format_xaxis))

ax2.set_xlabel('Payload Size (Log Scale)')
ax2.set_ylabel('Time (Microseconds, Log Scale)')
ax2.set_title('Latency Stability: Avg Latency vs Jitter')
ax2.legend()
ax2.grid(True, which="both", ls="-", alpha=0.2)

# 动态标注最后一个点的数值 (最大包)
last_row = df.iloc[-1]
ax2.annotate(f"Avg: {int(last_row['Avg_Lat_Micros'])}us", 
             xy=(last_row['Payload_Bytes'], last_row['Avg_Lat_Micros']),
             xytext=(0, -20), textcoords='offset points', ha='center', color='#2ca02c')
ax2.annotate(f"Jitter: {int(last_row['Jitter_Micros'])}us", 
             xy=(last_row['Payload_Bytes'], last_row['Jitter_Micros']),
             xytext=(0, 10), textcoords='offset points', ha='center', color='#d62728')

filename2 = 'chart2_latency_jitter.png'
plt.savefig(filename2, dpi=300)
print(f"生成图片: {filename2}")

# ========================================================
# 图表 3：QPS 处理能力 (System Overhead)
# ========================================================
fig3, ax3 = plt.subplots(figsize=(10, 6))
ax3.plot(df['Payload_Bytes'], df['QPS'], marker='D', color='#9467bd', label='QPS')

ax3.set_xscale('log')
ax3.set_yscale('log') # QPS 跨度很大
ax3.xaxis.set_major_formatter(ticker.FuncFormatter(format_xaxis))

ax3.set_xlabel('Payload Size (Log Scale)')
ax3.set_ylabel('Queries Per Second (Log Scale)')
ax3.set_title('gRPC Processing Capacity (QPS)')
ax3.legend()
ax3.grid(True, which="both", ls="-", alpha=0.2)

filename3 = 'chart3_qps.png'
plt.savefig(filename3, dpi=300)
print(f"生成图片: {filename3}")

print("\n所有图表生成完毕！")