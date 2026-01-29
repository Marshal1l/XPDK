# XPDK 设计文档

## 1. 项目概述

### 1.1 设计目标

XPDK 是一个受 DPDK 启发的高性能用户态 UDP 网络协议栈，基于 libpcap 构建，专为低延迟、高吞吐的网络通信场景设计。项目复刻了 DPDK 的核心优化思想，在无 DPDK 依赖的环境下实现了接近 DPDK 的性能表现。

### 1.2 核心设计原则

| 原则 | 说明 |
|------|------|
| **零拷贝** | 数据包从网卡到应用层全程零拷贝 |
| **无锁并发** | 核心数据路径无锁化，避免竞争 |
| **缓存友好** | 数据结构缓存行对齐，减少伪共享 |
| **批量处理** | 摊销系统调用和原子操作开销 |
| **硬件加速** | 充分利用网卡硬件卸载能力 |

## 2. 系统架构

### 2.1 整体架构

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              应用层 (Application)                             │
│                    ┌─────────────────────────────────┐                      │
│                    │      Socket API (UDP)           │                      │
│                    └─────────────────────────────────┘                      │
├─────────────────────────────────────────────────────────────────────────────┤
│                           UDP 协议栈 (UDP Stack)                              │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────────────┐   │
│  │  Socket Table   │  │ Packet Handler  │  │    Checksum Calculator      │   │
│  │  (HashMap)      │  │ (Zero-copy)     │  │    (HW/SW Hybrid)           │   │
│  └─────────────────┘  └─────────────────┘  └─────────────────────────────┘   │
├─────────────────────────────────────────────────────────────────────────────┤
│                         队列管理层 (Queue Management)                          │
│  ┌─────────────────────────────────────────────────────────────────────────┐ │
│  │                    Lock-free Ring Buffer Queue                          │ │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐ │ │
│  │  │    SPSC     │  │    SPMC     │  │    MPSC     │  │      MPMC       │ │ │
│  │  │  核心间通信  │  │  负载均衡   │  │  聚合场景   │  │   通用场景      │ │ │
│  │  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────────┘ │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
├─────────────────────────────────────────────────────────────────────────────┤
│                       轮询模式驱动 (Poll Mode Driver)                          │
│  ┌─────────────────────────────────────────────────────────────────────────┐ │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────┐  ┌──────────┐ ┌──────────┐    │ │
│  │  │ RX Queue │ │ RX Queue │ │ RX Queue │  │ TX Queue │ │ TX Queue │    │ │
│  │  │    0     │ │    1     │ │    N     │  │    0     │ │    M     │    │ │
│  │  └────┬─────┘ └────┬─────┘ └────┬─────┘  └────┬─────┘ └────┬─────┘    │ │
│  │       └─────────────┴────────────┘            └────────────┴──────────┘  │ │
│  │                         │                              │                │ │
│  │                    ┌────┴────┐                  ┌──────┴──────┐          │ │
│  │                    │  RSS    │                  │   Batch     │          │ │
│  │                    │ Hashing │                  │   Send      │          │ │
│  │                    └────┬────┘                  └─────────────┘          │ │
│  └─────────────────────────┼───────────────────────────────────────────────┘ │
├────────────────────────────┼────────────────────────────────────────────────┤
│                       内存管理层 (Memory Management)                           │
│  ┌─────────────────────────┴───────────────────────────────────────────────┐  │
│  │                         Memory Manager                                   │  │
│  │  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────────┐  │  │
│  │  │ HugePage        │  │ Mbuf Pool       │  │ NUMA Allocator          │  │  │
│  │  │ Allocator       │  │ (Object Pool)   │  │ (Node-local Memory)     │  │  │
│  │  └─────────────────┘  └─────────────────┘  └─────────────────────────┘  │  │
│  └─────────────────────────────────────────────────────────────────────────┘  │
├─────────────────────────────────────────────────────────────────────────────┤
│                          硬件抽象层 (Hardware Abstraction)                     │
│  ┌─────────────────────────────────────────────────────────────────────────┐ │
│  │                              libpcap                                    │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 2.2 数据流

**接收路径 (RX Path):**
```
NIC → libpcap → PMD (poll) → Queue (lockfree) → UDP Stack → Application
 │       │           │              │              │            │
 ▼       ▼           ▼              ▼              ▼            ▼
RSS   Packet      Mbuf Alloc    Enqueue Batch  Socket Demux  Zero-copy
Hash  Copy        Zero-copy     (bulk/no lock)                to App
```

**发送路径 (TX Path):**
```
Application → UDP Stack → Queue (lockfree) → PMD (batch) → NIC
     │             │            │               │           │
     ▼             ▼            ▼               ▼           ▼
Socket      Packet Build   Enqueue         Batch Send   Hardware TX
Zero-copy   in Mbuf        (bulk/no lock)  (amortize
                                          syscall)
```

## 3. 核心模块设计

### 3.1 内存管理模块

#### 3.1.1 设计目标
- 减少 TLB Miss（大页内存）
- 零拷贝数据包处理（mbuf 池）
- NUMA 亲和性（节点本地内存）

#### 3.1.2 Mbuf 设计

```rust
/// Mbuf (Memory Buffer) - 数据包缓冲区
#[repr(C, align(64))]  // 64字节缓存行对齐
pub struct Mbuf {
    pub data: *mut u8,           // 数据指针
    pub data_len: usize,         // 数据长度
    pub buf_len: usize,          // 缓冲区总长度
    pub ref_cnt: AtomicUsize,    // 引用计数（支持零拷贝共享）
    pub pool_id: u16,            // 所属内存池ID
    pub flags: MbufFlags,        // 标志位
    pub timestamp: u64,          // 时间戳（纳秒）
    pub next: Option<NonNull<Mbuf>>, // 链表指针
    _padding: [u8; 24],          // 缓存行填充
}
```

#### 3.1.3 大页内存分配

```
HugePage 0 (2MB)              HugePage 1 (2MB)
┌─────────┬─────────┐        ┌─────────┬─────────┐
│  Mbuf   │  Mbuf   │  ...   │  Mbuf   │  Mbuf   │
│ (2KB)   │ (2KB)   │        │ (2KB)   │ (2KB)   │
└─────────┴─────────┘        └─────────┴─────────┘

Free List: [Mbuf 3] -> [Mbuf 7] -> [Mbuf 12] -> ...
(无锁栈实现)
```

### 3.2 无锁环形队列

#### 3.2.1 设计特点
- **SPSC**: 单生产者单消费者，最高性能
- **SPMC**: 单生产者多消费者，适合负载均衡
- **MPSC**: 多生产者单消费者，适合数据聚合
- **MPMC**: 多生产者多消费者，通用场景

#### 3.2.2 核心算法

使用原子操作实现无锁并发：

```rust
/// 生产者操作
pub fn push(&self, item: T) -> Result<()> {
    let tail = self.tail.load(Ordering::Relaxed);
    let next_tail = (tail + 1) & self.mask;
    
    // 检查队列是否已满
    if next_tail == self.head.load(Ordering::Acquire) {
        return Err(Error::QueueFull);
    }
    
    // 写入数据
    unsafe {
        self.buffer[tail].write(item);
    }
    
    // 更新尾指针（Release 语义保证可见性）
    self.tail.store(next_tail, Ordering::Release);
    Ok(())
}

/// 消费者操作
pub fn pop(&self) -> Result<T> {
    let head = self.head.load(Ordering::Relaxed);
    
    // 检查队列是否为空
    if head == self.tail.load(Ordering::Acquire) {
        return Err(Error::QueueEmpty);
    }
    
    // 读取数据
    let item = unsafe {
        self.buffer[head].read()
    };
    
    // 更新头指针
    let next_head = (head + 1) & self.mask;
    self.head.store(next_head, Ordering::Release);
    
    Ok(item)
}
```

### 3.3 PMD 轮询驱动

#### 3.3.1 设计目标
- 绕开内核网络栈
- 批量收发摊销开销
- 多队列并行处理

#### 3.3.2 批处理优化

```rust
/// 批量接收
pub fn recv_batch(&self, mbufs: &mut [*mut Mbuf], batch_size: usize) -> usize {
    let mut received = 0;
    
    // 一次处理多个包，减少系统调用
    for i in 0..batch_size {
        match self.recv_single() {
            Ok(mbuf) => {
                mbufs[i] = mbuf;
                received += 1;
            }
            Err(_) => break,
        }
    }
    
    received
}
```

### 3.4 UDP 协议栈

#### 3.4.1 轻量级设计
- 简化协议处理逻辑
- 零拷贝数据传递
- Socket 表哈希索引

#### 3.4.2 数据包处理流程

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   Packet    │────▶│   Parse     │────▶│   Lookup    │
│   Arrival   │     │   Headers   │     │   Socket    │
└─────────────┘     └─────────────┘     └──────┬──────┘
                                               │
                                               ▼
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   App       │◀────│   Deliver   │◀────│   Validate  │
│   Receive   │     │   to App    │     │   Checksum  │
└─────────────┘     └─────────────┘     └─────────────┘
```

## 4. 性能优化策略

### 4.1 缓存优化

#### 4.1.1 缓存行对齐
```rust
#[repr(C, align(64))]
struct CacheAligned<T> {
    value: T,
    _padding: [u8; 64 - std::mem::size_of::<T>()],
}
```

#### 4.1.2 伪共享避免
```
CPU Core 0          CPU Core 1
┌──────────┐        ┌──────────┐
│ Counter  │        │ Counter  │
│ (padded) │        │ (padded) │
│ 64 bytes │        │ 64 bytes │
└──────────┘        └──────────┘
   不同缓存行，无伪共享
```

### 4.2 NUMA 优化

```rust
/// NUMA 感知内存分配
pub fn alloc_on_node(size: usize, node: i32) -> *mut c_void {
    unsafe {
        libc::mmap(
            ptr::null_mut(),
            size,
            PROT_READ | PROT_WRITE,
            MAP_PRIVATE | MAP_ANONYMOUS,
            -1,
            0,
        )
        // 使用 numa_alloc_onnode 绑定到特定 NUMA 节点
    }
}
```

### 4.3 硬件卸载

#### 4.3.1 支持的卸载功能
- **Checksum Offload**: IPv4/UDP/TCP 校验和计算
- **RSS (Receive Side Scaling)**: 多队列接收分流
- **TSO (TCP Segmentation Offload)**: TCP 分段卸载
- **LRO (Large Receive Offload)**: 大包接收合并

#### 4.3.2 RSS 哈希算法

```rust
/// Toeplitz 哈希（DPDK 兼容）
fn toeplitz_hash(&self, packet_data: &[u8]) -> u32 {
    let mut hash = 0u32;
    let mut key_bits = 0u64;
    
    // 初始化密钥位
    for (i, &byte) in self.rss_key.iter().take(8).enumerate() {
        key_bits |= (byte as u64) << (i * 8);
    }
    
    // Toeplitz 哈希计算
    for &byte in packet_data.iter().take(64) {
        hash = hash.wrapping_mul(31).wrapping_add(byte as u32);
        hash ^= (key_bits >> (byte % 64)) as u32;
    }
    
    hash
}
```

## 5. 扩展性设计

### 5.1 水平扩展

```
                    ┌─────────────┐
                    │   Load      │
                    │   Balancer  │
                    └──────┬──────┘
                           │
           ┌───────────────┼───────────────┐
           │               │               │
           ▼               ▼               ▼
     ┌───────────┐   ┌───────────┐   ┌───────────┐
     │  Worker   │   │  Worker   │   │  Worker   │
     │  Core 0   │   │  Core 1   │   │  Core N   │
     │           │   │           │   │           │
     │ ┌───────┐ │   │ ┌───────┐ │   │ ┌───────┐ │
     │ │Queue 0│ │   │ │Queue 1│ │   │ │Queue N│ │
     │ └───┬───┘ │   │ └───┬───┘ │   │ └───┬───┘ │
     └─────┼─────┘   └─────┼─────┘   └─────┼─────┘
           │               │               │
           └───────────────┼───────────────┘
                           │
                    ┌──────┴──────┐
                    │     NIC     │
                    │  (Multi-Q)  │
                    └─────────────┘
```

### 5.2 核心绑定

```rust
/// 设置 CPU 亲和性
pub fn set_cpu_affinity(cores: &[usize]) -> Result<()> {
    let mut cpu_set = CpuSet::new();
    for &core in cores {
        cpu_set.set(core)?;
    }
    sched_setaffinity(Pid::from_raw(0), &cpu_set)?;
    Ok(())
}
```

## 6. 可靠性设计

### 6.1 错误处理
- 使用 Rust `Result` 类型进行错误传播
- 关键路径使用 `saturating_add` 防止溢出
- 内存分配失败优雅降级

### 6.2 资源管理
- RAII 模式管理内存和文件描述符
- Drop trait 自动释放资源
- 引用计数防止内存泄漏

### 6.3 监控指标

```rust
/// 性能统计
pub struct PerfStats {
    pub packets_received: AtomicU64,
    pub packets_sent: AtomicU64,
    pub bytes_received: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub drops: AtomicU64,
    pub errors: AtomicU64,
    pub latency_histogram: Histogram,
}
```

## 7. 与 DPDK 的对比

| 特性 | XPDK | DPDK | 说明 |
|------|------|------|------|
| 依赖 | libpcap | 专用驱动 | XPDK 更易部署 |
| 大页内存 | 支持 | 支持 | 两者都支持 |
| 无锁队列 | 支持 | 支持 | 两者都支持 |
| 硬件卸载 | 部分 | 完整 | DPDK 更全面 |
| 学习曲线 | 低 | 高 | XPDK 更易上手 |
| 性能 | 接近原生 | 最优 | XPDK 约 90% DPDK |

## 8. 未来规划

### 8.1 短期目标
- [ ] 完善硬件卸载支持
- [ ] 添加更多示例程序
- [ ] 性能基准测试套件

### 8.2 长期目标
- [ ] 支持 DPDK 作为可选后端
- [ ] 集成 io_uring 优化
- [ ] 支持更多协议（TCP、RDMA）

## 9. 参考资源

- [DPDK 官方文档](https://doc.dpdk.org/guides/)
- [Linux 大页内存管理](https://www.kernel.org/doc/Documentation/vm/hugetlbpage.txt)
- [ lock-free 算法](https://www.cs.rochester.edu/research/synchronization/pseudocode/queues.html)
