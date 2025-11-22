import matplotlib.pyplot as plt
import pandas as pd
import matplotlib.ticker as ticker
import os
import sys

CSV_FILE_PATH = 'grpc_benchmark_results.csv'

if not os.path.exists(CSV_FILE_PATH):
    print(f"错误: 找不到文件 '{CSV_FILE_PATH}'")
    sys.exit(1)

try:
    df = pd.read_csv(CSV_FILE_PATH)
    print(f"成功读取数据: {len(df)} 行")
except Exception as e:
    print(f"读取 CSV 文件失败: {e}")
    sys.exit(1)

# ========================================================
# 数据预处理
# ========================================================
# 将所有 0 值替换为 1，防止对数坐标轴报错
# 注意：现在有了 TTFB_Micros (真·首字节) 和 First_Msg_Micros (首消息)
time_metrics = ['Avg_Lat_Micros', 'Jitter_Micros', 'TTFB_Micros', 'First_Msg_Micros', 'Max_Lat_Micros']
for metric in time_metrics:
    if metric in df.columns:
        df[metric] = df[metric].replace(0, 1)

# 设置风格
plt.style.use('seaborn-v0_8-whitegrid')
plt.rcParams.update({
    'font.family': 'serif',
    'font.size': 12,
    'axes.labelsize': 14,
    'axes.titlesize': 16,
    'xtick.labelsize': 12,
    'ytick.labelsize': 12,
    'legend.fontsize': 11,
    'lines.linewidth': 2,
    'lines.markersize': 6,
    'figure.autolayout': True
})

def format_xaxis(x, pos):
    if x < 1024: return f'{int(x)}B'
    elif x < 1024**2: return f'{int(x/1024)}KB'
    else: return f'{int(x/1024**2)}MB'

# ========================================================
# 图表 1：吞吐量 (Throughput)
# ========================================================
fig1, ax1 = plt.subplots(figsize=(10, 6))
ax1.plot(df['Payload_Bytes'], df['MBPS'], marker='o', color='#1f77b4', label='Throughput')

max_throughput = df['MBPS'].max()
ax1.axhline(y=max_throughput, color='r', linestyle='--', alpha=0.5, label=f'Peak (~{int(max_throughput)} MB/s)')

ax1.set_xscale('log')
ax1.xaxis.set_major_formatter(ticker.FuncFormatter(format_xaxis))
ax1.set_xlabel('Payload Size (Log Scale)')
ax1.set_ylabel('Throughput (MB/s)')
ax1.set_title('gRPC Throughput Analysis: The Software Bottleneck')
ax1.legend(loc='lower right')
ax1.grid(True, which="both", ls="-", alpha=0.2)
plt.savefig('chart1_throughput.png', dpi=300)
print("生成 Chart 1")

# ========================================================
# 图表 2：延迟综合分析 (Avg, Jitter, TTFB, FirstMsg, Max)
# ========================================================
fig2, ax2 = plt.subplots(figsize=(12, 8)) 

# 1. Max Latency (黑虚线)
ax2.plot(df['Payload_Bytes'], df['Max_Lat_Micros'], 
         marker='x', color='black', linestyle=':', alpha=0.7, label='Max Latency (Tail)')

# 2. First Message Latency 
ax2.plot(df['Payload_Bytes'], df['First_Msg_Micros'], 
         marker='o', color='#ff7f0e', linestyle='-.', label='1st Msg Latency (Cold Start)')

# 3. TTFB (青色实线 - 新增)
# 这条线应该很低，表示 Headers 握手很快
ax2.plot(df['Payload_Bytes'], df['TTFB_Micros'], 
         marker='*', color='#17becf', linestyle='-', label='True TTFB (Headers)')

# 4. Jitter (红虚线)
ax2.plot(df['Payload_Bytes'], df['Jitter_Micros'], 
         marker='^', color='#d62728', linestyle='--', label='Jitter (Std Dev)')

# 5. Avg Latency (绿实线)
ax2.plot(df['Payload_Bytes'], df['Avg_Lat_Micros'], 
         marker='s', color='#2ca02c', label='Avg Latency (Warm)')

ax2.set_xscale('log')
ax2.set_yscale('log') 
ax2.xaxis.set_major_formatter(ticker.FuncFormatter(format_xaxis))

ax2.set_xlabel('Payload Size (Log Scale)')
ax2.set_ylabel('Time (Microseconds, Log Scale)')
ax2.set_title('Latency Profile: Headers vs Body vs Tail')
ax2.legend(loc='upper left', frameon=True)
ax2.grid(True, which="both", ls="-", alpha=0.2)

# 动态标注 (取最后一个点)
last_row = df.iloc[-1]
x_pos = last_row['Payload_Bytes']

def add_label(ax, x, y, text, color, offset=(0, 0)):
    ax.annotate(text, xy=(x, y), xytext=offset, 
                textcoords='offset points', ha='left', va='center', 
                color=color, fontsize=9, weight='bold')

add_label(ax2, x_pos, last_row['Max_Lat_Micros'],   f" Max: {int(last_row['Max_Lat_Micros']/1000)}ms", 'black', (5, 0))
add_label(ax2, x_pos, last_row['First_Msg_Micros'], f" 1st Msg: {int(last_row['First_Msg_Micros']/1000)}ms", '#ff7f0e', (5, -10))
add_label(ax2, x_pos, last_row['Jitter_Micros'],    f" Jitter: {int(last_row['Jitter_Micros'])}us", '#d62728', (5, -20))
add_label(ax2, x_pos, last_row['Avg_Lat_Micros'],   f" Avg: {int(last_row['Avg_Lat_Micros'])}us", '#2ca02c', (5, -30))
# 给 True TTFB 也加个标注，预期它很低
add_label(ax2, x_pos, last_row['TTFB_Micros'],      f" TTFB: {int(last_row['TTFB_Micros'])}us", '#17becf', (5, 10))

plt.savefig('chart2_latency_full.png', dpi=300)
print("生成 Chart 2")

# ========================================================
# 图表 3：QPS
# ========================================================
fig3, ax3 = plt.subplots(figsize=(10, 6))
ax3.plot(df['Payload_Bytes'], df['QPS'], marker='D', color='#9467bd', label='QPS')
ax3.set_xscale('log')
ax3.set_yscale('log')
ax3.xaxis.set_major_formatter(ticker.FuncFormatter(format_xaxis))
ax3.set_xlabel('Payload Size (Log Scale)')
ax3.set_ylabel('QPS (Log Scale)')
ax3.set_title('gRPC Processing Capacity (QPS)')
ax3.legend()
ax3.grid(True, which="both", ls="-", alpha=0.2)
plt.savefig('chart3_qps.png', dpi=300)
print("生成 Chart 3")

print("\n完成！")