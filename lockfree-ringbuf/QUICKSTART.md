# 快速开始指南

这是一个高性能的无锁环队列库，支持多种生产者-消费者模式。

## 基本使用

### 1. 添加依赖

```toml
[dependencies]
lockfree-ringbuf = "0.1.0"
```

### 2. 选择合适的队列类型

```rust
use lockfree_ringbuf::{SpscRingBuffer, MpscRingBuffer, SpmcRingBuffer, MpmcRingBuffer};

// 单生产者单消费者 (最快)
let spsc = SpscRingBuffer::<i32>::new(1024);

// 多生产者单消费者 (工作队列)
let mpsc = MpscRingBuffer::<i32>::new(1024);

// 单生产者多消费者 (广播)
let spmc = SpmcRingBuffer::<i32>::new(1024);

// 多生产者多消费者 (通用队列)
let mpmc = MpmcRingBuffer::<i32>::new(1024);
```

### 3. 基本操作

```rust
// 推入元素
queue.push(42).unwrap();

// 弹出元素
if let Ok(value) = queue.pop() {
    println!("Got: {}", value);
}

// 检查状态
println!("Empty: {}", queue.is_empty());
println!("Full: {}", queue.is_full());
println!("Length: {}", queue.len());
```

### 4. 批量操作

```rust
use lockfree_ringbuf::BatchOps;

// 批量推入
let items = [1, 2, 3, 4, 5];
queue.push_batch(&items).unwrap();

// 批量弹出
let mut buffer = [0; 10];
let count = queue.pop_batch(&mut buffer).unwrap();
println!("Got {} items: {:?}", count, &buffer[..count]);
```

## 性能特点

- **SPSC**: 最快，无原子操作开销
- **MPSC/SPMC**: 中等，使用CAS操作
- **MPMC**: 较慢，但支持完全并发
- **批量操作**: 显著提高吞吐量

## 线程安全

每种队列类型都有特定的线程安全保证：

- **SPSC**: 只能有一个生产者和一个消费者
- **MPSC**: 多个生产者，一个消费者
- **SPMC**: 一个生产者，多个消费者
- **MPMC**: 多个生产者和消费者

⚠️ 违反这些约定会导致数据竞争！

## 示例

查看 `examples/` 目录获取完整示例：

```bash
cargo run --example demo          # 基本演示
cargo run --example spsc_example  # SPSC示例
cargo run --example mpsc_example  # MPSC示例
cargo run --example spmc_example  # SPMC示例
cargo run --example mpmc_example  # MPMC示例
cargo run --example batch_example # 批量操作示例
```