//! Logging utilities for XPDK

use log::{Level, LevelFilter, Log, Metadata, Record};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// XPDK logger
pub struct XpdkLogger {
    /// Log writers
    writers: Mutex<Vec<Box<dyn LogWriter + Send + Sync>>>,
    /// Minimum log level
    level: AtomicUsize,
    /// Logger statistics
    stats: LoggerStats,
}

/// Logger statistics
#[derive(Debug, Default)]
pub struct LoggerStats {
    pub total_logs: AtomicUsize,
    pub error_logs: AtomicUsize,
    pub warn_logs: AtomicUsize,
    pub info_logs: AtomicUsize,
    pub debug_logs: AtomicUsize,
    pub trace_logs: AtomicUsize,
    pub dropped_logs: AtomicUsize,
}

/// Log writer trait
pub trait LogWriter {
    /// Write a log entry
    fn write(&mut self, record: &Record) -> std::io::Result<()>;

    /// Flush the writer
    fn flush(&mut self) -> std::io::Result<()>;

    /// Check if writer accepts this log level
    fn accepts(&self, level: Level) -> bool;
}

/// Console log writer
pub struct ConsoleWriter {
    /// Minimum level
    level: Level,
}

impl ConsoleWriter {
    /// Create a new console writer
    pub fn new(level: Level) -> Self {
        Self { level }
    }
}

impl LogWriter for ConsoleWriter {
    fn write(&mut self, record: &Record) -> std::io::Result<()> {
        if !self.accepts(record.level()) {
            return Ok(());
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();

        let level_color = match record.level() {
            Level::Error => "\x1b[31m", // Red
            Level::Warn => "\x1b[33m",  // Yellow
            Level::Info => "\x1b[32m",  // Green
            Level::Debug => "\x1b[36m", // Cyan
            Level::Trace => "\x1b[37m", // White
        };

        let reset = "\x1b[0m";

        println!(
            "{}[{}][{}]{} {} - {}{}",
            level_color,
            timestamp,
            record.level(),
            reset,
            record.target(),
            record.args(),
            reset
        );

        Ok(())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        // Console is typically unbuffered
        Ok(())
    }

    fn accepts(&self, level: Level) -> bool {
        level <= self.level
    }
}

/// File log writer
pub struct FileWriter {
    /// File writer
    writer: BufWriter<File>,
    /// Minimum level
    level: Level,
}

impl FileWriter {
    /// Create a new file writer
    pub fn new(path: &str, level: Level) -> std::io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;

        Ok(Self {
            writer: BufWriter::new(file),
            level,
        })
    }

    /// Create a new file writer with rotation
    pub fn new_with_rotation(path: &str, level: Level, _max_size: u64) -> std::io::Result<Self> {
        // For now, just create a regular file writer
        // In a real implementation, you would implement log rotation
        Self::new(path, level)
    }
}

impl LogWriter for FileWriter {
    fn write(&mut self, record: &Record) -> std::io::Result<()> {
        if !self.accepts(record.level()) {
            return Ok(());
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();

        let line = format!(
            "[{}][{}] {} - {}\n",
            timestamp,
            record.level(),
            record.target(),
            record.args()
        );

        self.writer.write_all(line.as_bytes())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }

    fn accepts(&self, level: Level) -> bool {
        level <= self.level
    }
}

/// Ring buffer log writer for high-performance logging
pub struct RingBufferWriter {
    /// Ring buffer for log messages
    buffer: lockfree_ringbuf::MpmcRingBuffer<String>,
    /// Maximum log level
    level: Level,
    /// Worker thread handle
    worker_handle: Option<std::thread::JoinHandle<()>>,
    /// Shutdown flag
    shutdown: Arc<AtomicBool>,
}

impl RingBufferWriter {
    /// Create a new ring buffer writer
    pub fn new(level: Level, buffer_size: usize) -> Self {
        let buffer: lockfree_ringbuf::MpmcRingBuffer<String> =
            lockfree_ringbuf::MpmcRingBuffer::new(buffer_size);
        let shutdown = Arc::new(AtomicBool::new(false));

        // Start worker thread
        let worker_buffer = buffer.clone();
        let worker_shutdown = Arc::clone(&shutdown);
        let worker_handle = std::thread::spawn(move || {
            let mut file_writer = match FileWriter::new("xpdk.log", level) {
                Ok(writer) => writer,
                Err(_) => return,
            };

            while !worker_shutdown.load(Ordering::Relaxed) {
                match worker_buffer.pop() {
                    Ok(message) => {
                        // Parse the message and write to file
                        // This is a simplified implementation
                        let _ = file_writer.writer.write_all(message.as_bytes());
                    }
                    Err(_) => {
                        // No messages available
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                }
            }
        });

        Self {
            buffer,
            level,
            worker_handle: Some(worker_handle),
            shutdown,
        }
    }
}

impl LogWriter for RingBufferWriter {
    fn write(&mut self, record: &Record) -> std::io::Result<()> {
        if !self.accepts(record.level()) {
            return Ok(());
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();

        let message = format!(
            "[{}][{}] {} - {}\n",
            timestamp,
            record.level(),
            record.target(),
            record.args()
        );

        if let Err(_) = self.buffer.push(message) {
            // Buffer full, drop the message
        }

        Ok(())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        // Ring buffer doesn't need explicit flushing
        Ok(())
    }

    fn accepts(&self, level: Level) -> bool {
        level <= self.level
    }
}

impl Drop for RingBufferWriter {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
    }
}

impl XpdkLogger {
    /// Create a new XPDK logger
    pub fn new() -> Self {
        Self {
            writers: Mutex::new(Vec::new()),
            level: AtomicUsize::new(Level::Info as usize),
            stats: LoggerStats::default(),
        }
    }

    /// Add a log writer
    pub fn add_writer(&self, writer: Box<dyn LogWriter + Send + Sync>) {
        let mut writers = self.writers.lock().unwrap();
        writers.push(writer);
    }

    /// Add console writer
    pub fn add_console_writer(&self, level: Level) {
        let writer = ConsoleWriter::new(level);
        self.add_writer(Box::new(writer));
    }

    /// Add file writer
    pub fn add_file_writer(&self, path: &str, level: Level) -> std::io::Result<()> {
        let writer = FileWriter::new(path, level)?;
        self.add_writer(Box::new(writer));
        Ok(())
    }

    /// Add ring buffer writer
    pub fn add_ring_buffer_writer(&self, level: Level, buffer_size: usize) {
        let writer = RingBufferWriter::new(level, buffer_size);
        self.add_writer(Box::new(writer));
    }

    /// Set minimum log level
    pub fn set_level(&self, level: Level) {
        self.level.store(level as usize, Ordering::Relaxed);
    }

    /// Get logger statistics
    pub fn stats(&self) -> LoggerStatsView {
        LoggerStatsView {
            total_logs: self.stats.total_logs.load(Ordering::Relaxed),
            error_logs: self.stats.error_logs.load(Ordering::Relaxed),
            warn_logs: self.stats.warn_logs.load(Ordering::Relaxed),
            info_logs: self.stats.info_logs.load(Ordering::Relaxed),
            debug_logs: self.stats.debug_logs.load(Ordering::Relaxed),
            trace_logs: self.stats.trace_logs.load(Ordering::Relaxed),
            dropped_logs: self.stats.dropped_logs.load(Ordering::Relaxed),
        }
    }
}

impl Log for XpdkLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() as usize <= self.level.load(Ordering::Relaxed)
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        // Update statistics
        self.stats.total_logs.fetch_add(1, Ordering::Relaxed);

        match record.level() {
            Level::Error => self.stats.error_logs.fetch_add(1, Ordering::Relaxed),
            Level::Warn => self.stats.warn_logs.fetch_add(1, Ordering::Relaxed),
            Level::Info => self.stats.info_logs.fetch_add(1, Ordering::Relaxed),
            Level::Debug => self.stats.debug_logs.fetch_add(1, Ordering::Relaxed),
            Level::Trace => self.stats.trace_logs.fetch_add(1, Ordering::Relaxed),
        };

        // Write to all writers
        let mut writers = self.writers.lock().unwrap();
        for writer in writers.iter_mut() {
            if writer.accepts(record.level()) {
                if let Err(_) = writer.write(record) {
                    self.stats.dropped_logs.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    fn flush(&self) {
        let mut writers = self.writers.lock().unwrap();
        for writer in writers.iter_mut() {
            let _ = writer.flush();
        }
    }
}

/// Logger statistics view
#[derive(Debug)]
pub struct LoggerStatsView {
    pub total_logs: usize,
    pub error_logs: usize,
    pub warn_logs: usize,
    pub info_logs: usize,
    pub debug_logs: usize,
    pub trace_logs: usize,
    pub dropped_logs: usize,
}

/// Initialize XPDK logger
pub fn init_logger() -> Result<(), Box<dyn std::error::Error>> {
    let logger = XpdkLogger::new();

    // Add console writer for errors and warnings
    logger.add_console_writer(Level::Warn);

    // Add file writer for all logs
    logger.add_file_writer("xpdk.log", Level::Debug)?;

    // Add high-performance ring buffer writer
    logger.add_ring_buffer_writer(Level::Trace, 65536);

    // Set logger as global logger
    log::set_boxed_logger(Box::new(logger))?;
    log::set_max_level(LevelFilter::Trace);

    Ok(())
}

/// Performance logger for timing operations
pub struct PerfLogger {
    /// Operation name
    operation: String,
    /// Start time
    start_time: std::time::Instant,
}

impl PerfLogger {
    /// Create a new performance logger
    pub fn new(operation: &str) -> Self {
        Self {
            operation: operation.to_string(),
            start_time: std::time::Instant::now(),
        }
    }

    /// Log performance metric
    pub fn log(self) {
        let elapsed = self.start_time.elapsed();
        log::info!(
            "Performance: {} took {} microseconds",
            self.operation,
            elapsed.as_micros()
        );
    }

    /// Log with custom level
    pub fn log_with_level(self, level: Level) {
        let elapsed = self.start_time.elapsed();
        let message = format!(
            "Performance: {} took {} microseconds",
            self.operation,
            elapsed.as_micros()
        );

        log::log!(level, "{}", message);
    }
}

/// Macro for performance logging
#[macro_export]
macro_rules! perf_log {
    ($operation:expr) => {
        let _perf = $crate::utils::logging::PerfLogger::new($operation);
        // Log at the end of scope
        _perf.log();
    };
    ($operation:expr, $level:expr) => {
        let _perf = $crate::utils::logging::PerfLogger::new($operation);
        // Log at the end of scope with custom level
        _perf.log_with_level($level);
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_console_writer() {
        let mut writer = ConsoleWriter::new(Level::Info);
        let record = log::Record::builder()
            .level(Level::Info)
            .target("test")
            .args(format_args!("Test message"))
            .build();

        assert!(writer.write(&record).is_ok());
        assert!(writer.flush().is_ok());
        assert!(writer.accepts(Level::Info));
        assert!(!writer.accepts(Level::Debug));
    }

    #[test]
    fn test_file_writer() {
        let mut writer = FileWriter::new("/tmp/xpdk_test.log", Level::Info).unwrap();
        let record = log::Record::builder()
            .level(Level::Info)
            .target("test")
            .args(format_args!("Test message"))
            .build();

        assert!(writer.write(&record).is_ok());
        assert!(writer.flush().is_ok());
    }

    #[test]
    fn test_logger_stats() {
        let stats = LoggerStats::default();
        assert_eq!(stats.total_logs.load(Ordering::Relaxed), 0);

        stats.total_logs.fetch_add(1, Ordering::Relaxed);
        assert_eq!(stats.total_logs.load(Ordering::Relaxed), 1);
    }
}
