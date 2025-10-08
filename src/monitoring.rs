use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Represents a point-in-time snapshot of task metrics
#[derive(Debug, Clone)]
pub struct TaskMetrics {
    /// Total number of tasks processed
    pub total_tasks: u64,
    /// Number of currently active tasks
    pub active_tasks: usize,
    /// Number of completed tasks
    pub completed_tasks: u64,
    /// Number of failed tasks
    pub failed_tasks: u64,
    /// Number of cancelled tasks
    pub cancelled_tasks: u64,
    /// Average task execution time in milliseconds
    pub avg_execution_time_ms: f64,
    /// Maximum task execution time in milliseconds
    pub max_execution_time_ms: u64,
    /// Number of task retries
    pub total_retries: u64,
    /// Timestamp when these metrics were collected
    pub collected_at: DateTime<Utc>,
    /// Peak memory usage in bytes
    pub peak_memory_bytes: u64,
    /// Peak CPU time in milliseconds
    pub peak_cpu_time_ms: u64,
    /// Maximum concurrent tasks seen
    pub peak_concurrent_tasks: u64,
}

/// Collector for task-related metrics
#[derive(Debug)]
pub struct TaskMetricsCollector {
    total_tasks: AtomicU64,
    active_tasks: AtomicUsize,
    completed_tasks: AtomicU64,
    failed_tasks: AtomicU64,
    cancelled_tasks: AtomicU64,
    total_execution_time_ms: AtomicU64,
    max_execution_time_ms: AtomicU64,
    total_retries: AtomicU64,
    peak_memory_bytes: AtomicU64,
    peak_cpu_time_ms: AtomicU64,
    peak_concurrent_tasks: AtomicU64,
}

impl Default for TaskMetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskMetricsCollector {
    pub fn new() -> Self {
        Self {
            total_tasks: AtomicU64::new(0),
            active_tasks: AtomicUsize::new(0),
            completed_tasks: AtomicU64::new(0),
            failed_tasks: AtomicU64::new(0),
            cancelled_tasks: AtomicU64::new(0),
            total_execution_time_ms: AtomicU64::new(0),
            max_execution_time_ms: AtomicU64::new(0),
            total_retries: AtomicU64::new(0),
            peak_memory_bytes: AtomicU64::new(0),
            peak_cpu_time_ms: AtomicU64::new(0),
            peak_concurrent_tasks: AtomicU64::new(0),
        }
    }

    /// Record the start of task execution
    pub fn record_task_start(&self) {
        self.total_tasks.fetch_add(1, Ordering::Relaxed);
        self.active_tasks.fetch_add(1, Ordering::Relaxed);
    }

    /// Record the completion of task execution
    pub fn record_task_completion(&self) {
        self.active_tasks.fetch_sub(1, Ordering::Relaxed);
        self.completed_tasks.fetch_add(1, Ordering::Relaxed);
        self.total_execution_time_ms.fetch_add(
            self.max_execution_time_ms.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
    }

    /// Record a task failure
    pub fn record_task_failure(&self) {
        self.active_tasks.fetch_sub(1, Ordering::Relaxed);
        self.failed_tasks.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a task cancellation
    pub fn record_task_cancellation(&self) {
        self.active_tasks.fetch_sub(1, Ordering::Relaxed);
        self.cancelled_tasks.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a task retry
    pub fn record_task_retry(&self) {
        self.total_retries.fetch_add(1, Ordering::Relaxed);
    }

    /// Update resource usage metrics
    pub fn update_resource_usage(&self, memory_bytes: u64, cpu_time_ms: u64) {
        // Update peak memory usage if current usage is higher
        loop {
            let current_peak = self.peak_memory_bytes.load(Ordering::Relaxed);
            if memory_bytes <= current_peak {
                break;
            }
            if self
                .peak_memory_bytes
                .compare_exchange(
                    current_peak,
                    memory_bytes,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                break;
            }
        }

        // Update peak CPU time if current usage is higher
        loop {
            let current_peak = self.peak_cpu_time_ms.load(Ordering::Relaxed);
            if cpu_time_ms <= current_peak {
                break;
            }
            if self
                .peak_cpu_time_ms
                .compare_exchange(
                    current_peak,
                    cpu_time_ms,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                break;
            }
        }

        // Update peak concurrent tasks if current count is higher
        let current_active = self.active_tasks.load(Ordering::Relaxed) as u64;
        loop {
            let current_peak = self.peak_concurrent_tasks.load(Ordering::Relaxed);
            if current_active <= current_peak {
                break;
            }
            if self
                .peak_concurrent_tasks
                .compare_exchange(
                    current_peak,
                    current_active,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                break;
            }
        }
    }

    /// Get the current metrics
    pub fn get_metrics(&self) -> TaskMetrics {
        let total_tasks = self.total_tasks.load(Ordering::Relaxed);
        let completed_tasks = self.completed_tasks.load(Ordering::Relaxed);
        let total_execution_time = self.total_execution_time_ms.load(Ordering::Relaxed);

        let avg_execution_time = if completed_tasks > 0 {
            total_execution_time as f64 / completed_tasks as f64
        } else {
            0.0
        };

        TaskMetrics {
            total_tasks,
            active_tasks: self.active_tasks.load(Ordering::Relaxed),
            completed_tasks,
            failed_tasks: self.failed_tasks.load(Ordering::Relaxed),
            cancelled_tasks: self.cancelled_tasks.load(Ordering::Relaxed),
            avg_execution_time_ms: avg_execution_time,
            max_execution_time_ms: self.max_execution_time_ms.load(Ordering::Relaxed),
            total_retries: self.total_retries.load(Ordering::Relaxed),
            collected_at: Utc::now(),
            peak_memory_bytes: self.peak_memory_bytes.load(Ordering::Relaxed),
            peak_cpu_time_ms: self.peak_cpu_time_ms.load(Ordering::Relaxed),
            peak_concurrent_tasks: self.peak_concurrent_tasks.load(Ordering::Relaxed),
        }
    }

    /// Update max execution time
    pub fn update_max_execution_time(&self, execution_time_ms: u64) {
        loop {
            let current_max = self.max_execution_time_ms.load(Ordering::Relaxed);
            if execution_time_ms <= current_max {
                break;
            }
            if self
                .max_execution_time_ms
                .compare_exchange(
                    current_max,
                    execution_time_ms,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                break;
            }
        }
    }
}

// Tool invocation metrics (executors)
#[derive(Debug)]
pub struct ToolMetricsCollector {
    invocations: AtomicU64,
    errors: AtomicU64,
    total_duration_ms: AtomicU64,
    max_duration_ms: AtomicU64,
    total_bytes: AtomicU64,
}

impl ToolMetricsCollector {
    pub fn new() -> Self {
        Self {
            invocations: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            total_duration_ms: AtomicU64::new(0),
            max_duration_ms: AtomicU64::new(0),
            total_bytes: AtomicU64::new(0),
        }
    }

    pub fn record(&self, duration_ms: u64, bytes: u64, is_error: bool) {
        self.invocations.fetch_add(1, Ordering::Relaxed);
        if is_error {
            self.errors.fetch_add(1, Ordering::Relaxed);
        }
        self.total_duration_ms
            .fetch_add(duration_ms, Ordering::Relaxed);
        // update max duration
        loop {
            let current = self.max_duration_ms.load(Ordering::Relaxed);
            if duration_ms <= current {
                break;
            }
            if self
                .max_duration_ms
                .compare_exchange(current, duration_ms, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }
        self.total_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> (u64, u64, u64, u64, u64) {
        (
            self.invocations.load(Ordering::Relaxed),
            self.errors.load(Ordering::Relaxed),
            self.total_duration_ms.load(Ordering::Relaxed),
            self.max_duration_ms.load(Ordering::Relaxed),
            self.total_bytes.load(Ordering::Relaxed),
        )
    }
}

pub static TOOL_METRICS: Lazy<ToolMetricsCollector> = Lazy::new(ToolMetricsCollector::new);

/// A guard that automatically records task completion time
pub struct TaskExecutionGuard {
    metrics: Arc<TaskMetricsCollector>,
    completed: bool,
    failed: bool,
    retried: bool,
    start_time: Instant,
}

impl TaskExecutionGuard {
    pub fn new(metrics: Arc<TaskMetricsCollector>) -> Self {
        metrics.record_task_start();
        Self {
            metrics,
            completed: false,
            failed: false,
            retried: false,
            start_time: Instant::now(),
        }
    }

    pub fn complete(&mut self) {
        self.completed = true;
    }

    pub fn fail(&mut self) {
        self.failed = true;
    }

    pub fn retry(&mut self) {
        self.retried = true;
    }

    /// Get the elapsed time in milliseconds since this guard was created
    pub fn elapsed_ms(&self) -> u64 {
        self.start_time.elapsed().as_millis() as u64
    }
}

impl Drop for TaskExecutionGuard {
    fn drop(&mut self) {
        let execution_time_ms = self.start_time.elapsed().as_millis() as u64;
        self.metrics.update_max_execution_time(execution_time_ms);

        if self.completed {
            self.metrics.record_task_completion();
        } else if self.failed {
            self.metrics.record_task_failure();
        } else if self.retried {
            self.metrics.active_tasks.fetch_sub(1, Ordering::Relaxed);
            self.metrics.record_task_retry();
        } else {
            // If no status was set, count it as a failure
            self.metrics.record_task_failure();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_metrics_collection() {
        let collector = Arc::new(TaskMetricsCollector::new());

        // Record a task start and completion
        {
            let mut _guard = TaskExecutionGuard::new(collector.clone());
            thread::sleep(Duration::from_millis(50));
            _guard.complete();
        }

        // Record a task failure
        {
            let mut _guard = TaskExecutionGuard::new(collector.clone());
            thread::sleep(Duration::from_millis(75));
            _guard.fail();
        }

        // Record a task retry
        {
            let mut _guard = TaskExecutionGuard::new(collector.clone());
            thread::sleep(Duration::from_millis(75));
            _guard.retry();
        }

        // Record some retries
        collector.record_task_retry();
        collector.record_task_retry();

        // Record a task that fails without explicit completion
        {
            let _guard = TaskExecutionGuard::new(collector.clone());
            // Guard is dropped without calling complete/fail/retry
        }

        // Get the metrics
        let metrics = collector.get_metrics();

        // Verify metrics
        assert_eq!(metrics.total_tasks, 4);
        assert_eq!(metrics.completed_tasks, 1);
        assert_eq!(metrics.failed_tasks, 2);
        assert_eq!(metrics.total_retries, 3);
    }

    #[test]
    fn test_guard_drop_behavior() {
        let collector = Arc::new(TaskMetricsCollector::new());

        // Test complete
        {
            let mut _guard = TaskExecutionGuard::new(collector.clone());
            _guard.complete();
        }

        // Test fail
        {
            let mut _guard = TaskExecutionGuard::new(collector.clone());
            _guard.fail();
        }

        // Test retry
        {
            let mut _guard = TaskExecutionGuard::new(collector.clone());
            _guard.retry();
        }

        // Test drop without calling any method
        {
            let _guard = TaskExecutionGuard::new(collector.clone());
            // Guard is dropped without calling complete/fail/retry
        }

        // Test drop after calling a method
        {
            let mut _guard = TaskExecutionGuard::new(collector.clone());
            _guard.complete();
            // Guard is dropped after calling complete
        }
    }
}
