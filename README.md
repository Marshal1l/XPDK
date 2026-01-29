# XPDK - 高性能用户态 UDP 网络协议栈

XPDK 是一个受 DPDK 启发的高性能用户态网络实现，基于 libpcap 构建，专为低延迟、高吞吐的网络通信场景设计。项目复刻了 DPDK 的核心优化思想，在无 DPDK 依赖的环境下实现了接近 DPDK 的性能表现。

## 核心特性

### 🚀 性能优化
- **零拷贝数据包处理**：基于 mbuf 内存池的零拷贝架构，消除内核态/用户态数据拷贝开销
- **大页内存管理**：使用 Linux HugePages (2MB) 减少 TLB Miss，提升内存访问效率
- **缓存行对齐**：所有核心数据结构按 64 字节缓存行对齐，避免伪共享
- **NUMA 亲和性**：支持 NUMA 感知的内存分配，优化多路服务器的内存访问

### 🔒 无锁并发
- **无锁环形队列**：独立的 lockfree-ringbuf 子项目，实现 SPSC/SPMC/MPSC/MPMC 多种无锁队列
- **批量操作**：支持批量入队/出队，摊销原子操作开销
- **核绑定优化**：支持 CPU 亲和性设置，实现任务与核心的绑定隔离

### 🌐 网络特性
- **PMD 轮询模式**：基于 libpcap 的轮询收发包驱动，绕开内核协议栈
- **多队列支持**：支持网卡多队列配置，结合 RSS 实现多核并行 I/O
- **硬件卸载**：支持网卡硬件卸载功能（校验和计算、RSS 哈希、时间戳）
- **UDP 协议栈**：轻量级 UDP 协议栈实现，简化处理逻辑

### 📊 可观测性
- **高精度计时**：支持 TSC、单调时钟等多种时间源，纳秒级精度
- **延迟追踪**：内置延迟统计追踪器，支持 P99/P95 延迟分析
- **性能计数器**：全面的性能统计，包括包速率、吞吐量、丢包率等

## 系统架构

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           应用层 (Application Layer)                          │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────────────┐   │
│  │   UDP Echo Server │  │   UDP Client    │  │   Performance Test         │   │
│  └─────────────────┘  └─────────────────┘  └─────────────────────────────┘   │
├─────────────────────────────────────────────────────────────────────────────┤
│                          UDP 协议栈 (UDP Stack)                               │
│  ┌─────────────────────────────────────────────────────────────────────────┐ │
│  │  Socket Management │ Packet Processing │ Checksum Offload              │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
├─────────────────────────────────────────────────────────────────────────────┤
│                      无锁队列层 (Lock-free Queues)                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │    SPSC     │  │    SPMC     │  │    MPSC     │  │        MPMC         │  │
│  │  (单生产者  │  │ (单生产者   │  │ (多生产者   │  │    (多生产者        │  │
│  │  单消费者)  │  │ 多消费者)   │  │ 单消费者)   │  │    多消费者)        │  │
│  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────────────┘  │
├─────────────────────────────────────────────────────────────────────────────┤
│                    轮询模式驱动 (Poll Mode Driver)                             │
│  ┌─────────────────────────────────────────────────────────────────────────┐ │
│  │  RX Queue 0 │ RX Queue 1 │ ... │ TX Queue 0 │ TX Queue 1 │ ...         │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
├─────────────────────────────────────────────────────────────────────────────┤
│                      内存管理层 (Memory Management)                            │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────────────┐   │
│  │   Huge Pages    │  │   Mbuf Pools    │  │   NUMA Allocation           │   │
│  │   (2MB pages)   │  │   (Zero-copy)   │  │   (Node-local memory)       │   │
│  └─────────────────┘  └─────────────────┘  └─────────────────────────────┘   │
├─────────────────────────────────────────────────────────────────────────────┤
│                         libpcap / 网卡驱动层                                   │
│  ┌─────────────────────────────────────────────────────────────────────────┐ │
│  │                    Packet Capture / Injection                           │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
├─────────────────────────────────────────────────────────────────────────────┤
│                          物理网卡 (Network Interface)                          │
│  ┌─────────────────────────────────────────────────────────────────────────┐ │
│  │              Multi-Queue NIC with RSS / Hardware Offload                │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────────┘
```

## 核心设计思想

### 1. 绕开内核协议栈
传统 Linux 网络栈需要经过复杂的协议分层、系统调用、中断处理等路径，延迟较高。XPDK 通过 libpcap 直接操作网卡，在用户态完成数据包处理，显著降低延迟。

### 2. 大页内存优化
标准 4KB 页表在大量内存访问时会产生频繁的 TLB Miss。XPDK 使用 2MB HugePages，大幅减少页表项数量，提升 TLB 命中率，特别适合大内存池场景。

### 3. 无锁并发设计
锁竞争是多核扩展性的主要瓶颈。XPDK 使用无锁环形队列（Lock-free Ring Buffer），通过原子操作实现多生产者/多消费者并发，消除锁竞争开销。

### 4. 批量处理摊销开销
单包处理的系统调用、原子操作开销较高。XPDK 采用批量处理模式，一次处理多个数据包，摊销开销，提升吞吐。

### 5. 硬件卸载加速
现代网卡支持硬件卸载功能（校验和计算、RSS 分流等）。XPDK 充分利用这些特性，减轻 CPU 负担，进一步降低延迟。

## 快速开始

### 环境要求

- **操作系统**: Linux 3.10+ (推荐 5.0+)
- **Rust 版本**: 1.70 或更高
- **依赖库**: libpcap-dev
- **权限**: root 权限（用于网卡操作）
- **硬件**: 支持多队列和 RSS 的网卡（推荐）

### 安装依赖

```bash
# Ubuntu/Debian
sudo apt-get update
sudo apt-get install -y libpcap-dev build-essential

# CentOS/RHEL/Fedora
sudo yum install -y libpcap-devel gcc make

# Arch Linux
sudo pacman -S libpcap base-devel
```

### 配置大页内存

```bash
# 临时配置（立即生效，重启后失效）
sudo sysctl -w vm.nr_hugepages=1024

# 永久配置（写入 /etc/sysctl.conf）
echo "vm.nr_hugepages=1024" | sudo tee -a /etc/sysctl.conf

# 验证配置
cat /proc/meminfo | grep HugePages
```

### 编译项目

```bash
# 克隆仓库
git clone https://github.com/yourusername/xpdk.git
cd xpdk

# 开发编译
cargo build

# 发布编译（优化性能）
cargo build --release

# 运行测试
cargo test
```

## 使用示例

### UDP Echo 服务器

最简单的入门示例，接收 UDP 数据包并原样返回：

```bash
# 语法: udp_echo_server <网卡接口> <端口>
sudo ./target/release/examples/udp_echo_server eth0 8080
```

### UDP 客户端

用于测试与服务器通信：

```bash
# 语法: udp_client <服务器IP> <端口> [本地端口] [网卡接口]
sudo ./target/release/examples/udp_client 192.168.1.100 8080 0 eth0
```

### 性能测试工具

提供三种测试模式：

```bash
# 1. 服务器模式
sudo ./target/release/examples/performance_test server 8080 eth0

# 2. 客户端模式
sudo ./target/release/examples/performance_test client 192.168.1.100 8080 eth0

# 3. 回环测试模式（单机测试）
sudo ./target/release/examples/performance_test loopback 8080 eth0
```

## 配置选项

XPDK 通过 [`Config`](src/lib.rs:63) 结构体进行配置：

```rust
use xpdk::{Config, Xpdk};

let config = Config {
    // 网卡接口名称
    interface: "eth0".to_string(),
    
    // 内存池配置
    pool_count: 4,           // 内存池数量
    pool_size: 8192,         // 每个池的 mbuf 数量
    
    // 队列配置
    rx_queue_count: 4,       // 接收队列数
    tx_queue_count: 4,       // 发送队列数
    rx_queue_size: 4096,     // 接收队列大小
    tx_queue_size: 4096,     // 发送队列大小
    
    // 功能开关
    enable_hugepages: true,  // 启用大页内存
    enable_numa: true,       // 启用 NUMA 亲和
    enable_offload: true,    // 启用硬件卸载
    
    // CPU 亲和性
    cpu_affinity: Some(vec![0, 1, 2, 3]),
    
    ..Default::default()
};

let xpdk = Xpdk::new(config)?;
```

## 项目结构

```
xpdk/
├── Cargo.toml              # 项目配置
├── README.md               # 本文件
├── DESIGN.md               # 详细设计文档
├── src/
│   ├── lib.rs              # 库入口
│   ├── memory/             # 内存管理模块
│   │   └── mod.rs          # HugePages, MbufPool
│   ├── poll/               # 轮询驱动模块
│   │   └── mod.rs          # PMD, RxQueue, TxQueue
│   ├── queue/              # 队列模块
│   │   └── mod.rs          # RingBuffer 包装层
│   ├── udp/                # UDP 协议栈
│   │   └── mod.rs          # UdpStack, UdpSocket
│   ├── utils/              # 工具模块
│   │   ├── cpu.rs          # CPU 亲和性
│   │   ├── numa.rs         # NUMA 支持
│   │   ├── offload.rs      # 硬件卸载
│   │   └── time.rs         # 高精度计时
│   ├── numa.rs             # NUMA 模块导出
│   └── offload.rs          # Offload 模块导出
├── lockfree-ringbuf/       # 无锁环形队列子项目
│   ├── src/
│   │   ├── lib.rs          # 库入口
│   │   ├── spsc.rs         # 单生产者单消费者队列
│   │   ├── spmc.rs         # 单生产者多消费者队列
│   │   ├── mpsc.rs         # 多生产者单消费者队列
│   │   └── mpmc.rs         # 多生产者多消费者队列
│   └── examples/           # 队列使用示例
└── examples/               # XPDK 使用示例
    └── src/bin/
        ├── udp_echo_server.rs
        ├── udp_client.rs
        └── performance_test.rs
```

## 性能优化建议

### 1. 大页内存配置
确保系统已配置足够的大页内存：
```bash
# 查看当前大页配置
cat /proc/meminfo | grep Huge

# 建议配置: 1024 个 2MB 大页 = 2GB 大页内存
sudo sysctl -w vm.nr_hugepages=1024
```

### 2. CPU 亲和性绑定
将 XPDK 线程绑定到特定 CPU 核心，避免上下文切换：
```rust
config.cpu_affinity = Some(vec![2, 3, 4, 5]); // 绑定到核心 2-5
```

### 3. 网卡多队列配置
启用网卡多队列和 RSS：
```bash
# 查看网卡队列数
ethtool -l eth0

# 设置队列数（需网卡支持）
sudo ethtool -L eth0 combined 4
```

### 4. 中断亲和性
将网卡中断绑定到特定 CPU 核心：
```bash
# 查看网卡中断号
cat /proc/interrupts | grep eth0

# 设置中断亲和性（示例：将中断 123 绑定到 CPU 0）
echo 1 | sudo tee /proc/irq/123/smp_affinity
```

### 5. 禁用节能模式
CPU 节能模式会影响延迟表现：
```bash
# 临时禁用（性能模式）
sudo cpupower frequency-set -g performance

# 或针对特定核心
echo performance | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor
```

## 调试与监控

### 日志级别
通过环境变量设置日志级别：
```bash
RUST_LOG=debug sudo ./target/release/examples/udp_echo_server eth0 8080
```

### 性能分析
使用 `perf` 进行性能分析：
```bash
# 记录性能数据
sudo perf record -g ./target/release/examples/udp_echo_server eth0 8080

# 生成火焰图
sudo perf script | inferno-collapse-perf | inferno-flamegraph > flame.svg
```

## 贡献指南

欢迎提交 Issue 和 PR！请确保：

1. 代码通过 `cargo fmt` 格式化
2. 代码通过 `cargo clippy` 检查
3. 所有测试通过 `cargo test`
4. 提交信息清晰描述变更

## 许可证

本项目采用 MIT 或 Apache-2.0 双许可证，详见 [LICENSE-MIT](LICENSE-MIT) 和 [LICENSE-APACHE](LICENSE-APACHE)。

## 致谢

- 受 [DPDK](https://www.dpdk.org/) 项目启发
- 使用 [libpcap](https://www.tcpdump.org/) 进行数据包捕获
- 无锁队列算法参考 [DPDK Ring Library](https://doc.dpdk.org/guides/prog_guide/ring_lib.html)

## 相关资源

- [DPDK 官方文档](https://doc.dpdk.org/guides/)
- [Linux 大页内存管理](https://www.kernel.org/doc/Documentation/vm/hugetlbpage.txt)
- [Rust 异步编程](https://rust-lang.github.io/async-book/)
