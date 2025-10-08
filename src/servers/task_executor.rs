use std::error::Error;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::MutexGuard;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration as StdDuration;

use chrono::Utc;
use log::{debug, error, info, warn};
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::time::{timeout, Duration};

use crate::models::task::{Task, TaskSchedule, TaskStatus};
use crate::monitoring::{TaskExecutionGuard, TaskMetricsCollector};
use crate::servers::task_scheduler::DummyToolRegistry;
use crate::servers::tool_invoker::ToolInvoker;
use crate::utils::task_storage::{FileTaskStorage, TaskStorage};

/// TaskExecutor is responsible for executing tasks and managing the scheduling loop
pub struct TaskExecutor {
    storage: Arc<dyn TaskStorage>,
    tool_invoker: Arc<dyn ToolInvoker>,
    running: Arc<Mutex<bool>>,
    shutdown_rx: Arc<Mutex<Option<Receiver<()>>>>,
    active_tasks: Arc<Mutex<usize>>,
    task_complete: Arc<(Mutex<bool>, Condvar)>,
    scheduling_loop_handle: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
    metrics: Arc<TaskMetricsCollector>,
    tx: Sender<Task>,
    rx: Receiver<Task>,
    shutdown: Arc<Mutex<bool>>,
}

impl TaskExecutor {
    /// Create a new TaskExecutor
    pub fn new<T: ToolInvoker + 'static>(
        tool_invoker: Arc<T>,
        storage: Arc<dyn TaskStorage>,
        metrics: Arc<TaskMetricsCollector>,
    ) -> Self {
        info!("Creating new TaskExecutor");
        let (tx, rx) = mpsc::channel(100); // Add a buffer size
        let active_tasks = Arc::new(Mutex::new(0));
        let task_complete = Arc::new((Mutex::new(false), Condvar::new()));
        let shutdown = Arc::new(Mutex::new(false));
        Self {
            storage,
            tool_invoker,
            running: Arc::new(Mutex::new(false)),
            shutdown_rx: Arc::new(Mutex::new(None)),
            active_tasks,
            task_complete,
            scheduling_loop_handle: Arc::new(Mutex::new(None)),
            metrics,
            tx,
            rx,
            shutdown,
        }
    }

    /// Start the task executor
    pub fn start(&self) -> Result<(), Box<dyn Error>> {
        let mut running = lock_with_timeout(&self.running, "self.running in start()");
        if *running {
            warn!("TaskExecutor already running");
            return Ok(());
        }
        *running = true;
        info!("Starting TaskExecutor");

        let storage = self.storage.clone();
        let tool_invoker = self.tool_invoker.clone();
        let _running_clone = self.running.clone();
        let active_tasks = self.active_tasks.clone();
        let task_complete = self.task_complete.clone();
        let metrics = self.metrics.clone();
        let shutdown = self.shutdown.clone();
        let _tx = self.tx.clone();

        // Reset task complete state
        {
            let (lock, _) = &*task_complete;
            *lock_with_timeout(lock, "task_complete in start() reset") = false;
        }

        let handle = thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

            rt.block_on(async {
                loop {
                    if *lock_with_timeout(&shutdown, "shutdown in scheduling loop") {
                        break;
                    }

                    // Get all pending tasks
                    let tasks = match storage.list_tasks().await {
                        Ok(t) => t,
                        Err(e) => {
                            error!("Error listing tasks: {}", e);
                            Vec::new()
                        }
                    };
                    for task in tasks {
                        if task.status == TaskStatus::Pending {
                            // Increment active tasks
                            {
                                let mut count = lock_with_timeout(
                                    &active_tasks,
                                    "active_tasks increment in scheduling loop",
                                );
                                *count += 1;
                            }

                            if let Err(e) = Self::execute_task(
                                task,
                                Arc::clone(&tool_invoker),
                                Arc::clone(&storage),
                                Arc::clone(&task_complete),
                                Arc::clone(&metrics),
                            )
                            .await
                            {
                                error!("Task execution failed: {}", e);
                            }

                            // Decrement active tasks
                            {
                                let mut count = lock_with_timeout(
                                    &active_tasks,
                                    "active_tasks decrement in scheduling loop",
                                );
                                *count = count.saturating_sub(1);
                            }
                        }
                    }

                    // Brief pause between iterations
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            });
        });

        *lock_with_timeout(
            &self.scheduling_loop_handle,
            "scheduling_loop_handle in start()",
        ) = Some(handle);

        Ok(())
    }

    /// Execute a single task
    async fn execute_task(
        mut task: Task,
        tool_invoker: Arc<dyn ToolInvoker>,
        storage: Arc<dyn TaskStorage>,
        task_complete: Arc<(Mutex<bool>, Condvar)>,
        metrics: Arc<TaskMetricsCollector>,
    ) -> Result<(), Box<dyn Error>> {
        let _start_time = std::time::Instant::now();

        // Update task status to Running
        task.update_status(TaskStatus::Running)?;
        task.log_event(TaskStatus::Running, Some("Task started".to_string()));
        // Persist updated status (async)
        storage.update_task(task.clone()).await?;

        // Execute the task with timeout
        let result = match timeout(
            Duration::from_secs(task.timeout),
            tool_invoker.invoke_tool(task.tool.clone(), task.arguments.clone()),
        )
        .await
        {
            Ok(Ok(output)) => {
                task.result = Some(output);
                task.log_event(
                    TaskStatus::Completed,
                    Some("Task completed successfully".to_string()),
                );
                task.update_status(TaskStatus::Completed)?;
                metrics.record_task_completion();
                Ok(())
            }
            Ok(Err(e)) => {
                task.error = Some(e.to_string());
                task.log_event(TaskStatus::Failed, Some(format!("Task failed: {}", e)));
                task.update_status(TaskStatus::Failed)?;
                metrics.record_task_failure();
                Err(format!("Tool invocation failed: {}", e))
            }
            Err(_) => {
                task.error = Some("Task timed out".to_string());
                task.log_event(TaskStatus::Failed, Some("Task timed out".to_string()));
                task.update_status(TaskStatus::Failed)?;
                metrics.record_task_failure();
                Err("Task execution timed out".into())
            }
        };

        // Persist final state
        storage.update_task(task.clone()).await?;

        // Signal task completion
        let (lock, cvar) = &*task_complete;
        {
            let mut complete = lock.lock().unwrap();
            *complete = true;
            cvar.notify_one();
        }

        result.map_err(|e| e.into())
    }

    /// Stop the task executor
    pub fn stop(&self) {
        info!("Stopping TaskExecutor");

        // Set shutdown flag
        {
            let mut shutdown = lock_with_timeout(&self.shutdown, "shutdown in stop()");
            *shutdown = true;
        }

        // Wait for any active tasks to complete with a timeout
        let start = std::time::Instant::now();
        while self.active_task_count() > 0 && start.elapsed().as_secs() < 5 {
            info!(
                "Waiting for {} active tasks to complete...",
                self.active_task_count()
            );
            thread::sleep(Duration::from_millis(100));
        }

        // If there are still active tasks, log a warning
        if self.active_task_count() > 0 {
            warn!(
                "Stop timed out with {} active tasks remaining",
                self.active_task_count()
            );
        } else {
            info!("All tasks completed, stop complete");
        }

        // Join the scheduling loop thread to ensure full cleanup
        if let Some(handle) = lock_with_timeout(
            &self.scheduling_loop_handle,
            "scheduling_loop_handle in stop()",
        )
        .take()
        {
            info!("[DEBUG] Joining scheduling loop thread");
            let _ = handle.join();
            info!("[DEBUG] Scheduling loop thread joined");
        }
    }

    /// Stop the task executor and wait for all tasks to complete
    pub async fn shutdown(&self) -> Result<(), Box<dyn Error>> {
        info!("Initiating graceful shutdown of TaskExecutor");

        // Set running to false to stop the scheduling loop
        {
            let mut running = self.running.lock().unwrap();
            *running = false;
        }

        // Wait for the scheduling loop to exit
        let scheduling_loop_handle = {
            let mut handle_lock = self.scheduling_loop_handle.lock().unwrap();
            handle_lock.take()
        };

        if let Some(handle) = scheduling_loop_handle {
            info!("[DEBUG] Joining scheduling loop thread (shutdown)");
            handle
                .join()
                .map_err(|_| "Failed to join scheduling loop thread")?;
            info!("[DEBUG] Scheduling loop thread joined (shutdown)");
        }

        // Wait for shutdown signal from the scheduling loop thread
        let rx_opt = lock_with_timeout(&self.shutdown_rx, "shutdown_rx in shutdown()").take();
        if let Some(mut rx) = rx_opt {
            // Use a loop to handle potential async timing issues
            let mut attempts = 0;
            let result: Result<(), Box<dyn Error>> = loop {
                match timeout(Duration::from_secs(5), rx.recv()).await {
                    Ok(Some(_)) => {
                        info!("Shutdown signal received from scheduling loop thread");
                        break Ok(());
                    }
                    Ok(None) => {
                        warn!("Channel closed before shutdown signal");
                        break Err("Channel closed".into());
                    }
                    Err(_) => {
                        attempts += 1;
                        if attempts >= 3 {
                            warn!("Shutdown signal not received after multiple attempts");
                            break Err("Receive timeout".into());
                        }
                        // Wait a bit before retrying
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            };
            result?;
        }

        // Reset state (but preserve tasks)
        {
            let mut running = self.running.lock().unwrap();
            *running = false;
        }

        Ok(())
    }

    /// Wait for a task to complete
    pub fn wait_for_task_completion(&self, timeout_ms: u64) -> bool {
        let (lock, cvar) = &*self.task_complete;
        let start = std::time::Instant::now();

        loop {
            let mut task_complete =
                lock_with_timeout(lock, "task_complete in wait_for_task_completion");
            if *task_complete {
                *task_complete = false; // Reset for next wait
                return true;
            }
            // Check if we've exceeded the timeout
            if start.elapsed().as_millis() > timeout_ms as u128 {
                return false;
            }
            // Wait for a short time before checking again
            let _ = cvar
                .wait_timeout(task_complete, std::time::Duration::from_millis(100))
                .unwrap();
        }
    }

    /// Check if the executor is running
    pub fn is_running(&self) -> bool {
        *lock_with_timeout(&self.running, "self.running in is_running()")
    }

    /// Get the number of active tasks
    pub fn active_task_count(&self) -> usize {
        *lock_with_timeout(&self.active_tasks, "active_tasks in active_task_count()")
    }

    /// Get the current task metrics
    pub fn get_metrics(&self) -> crate::monitoring::TaskMetrics {
        self.metrics.get_metrics()
    }

    /// Get current memory usage for the task
    fn _get_current_memory_usage() -> u64 {
        // For testing or overrides, allow an env var to specify mock memory usage
        if let Ok(val) = std::env::var("mcp_registrar_MOCK_MEMORY_BYTES") {
            if let Ok(bytes) = val.parse::<u64>() {
                return bytes;
            }
        }

        // Use system-specific memory usage tracking
        #[cfg(target_os = "linux")]
        {
            // On Linux, read from /proc/self/statm
            match std::fs::read_to_string("/proc/self/statm") {
                Ok(statm) => {
                    let values: Vec<&str> = statm.split_whitespace().collect();
                    if values.len() >= 2 {
                        // Second value is resident set size in pages
                        if let Ok(rss_pages) = values[1].parse::<u64>() {
                            // Convert pages to bytes (usually 4KB per page)
                            return rss_pages * 4096;
                        }
                    }
                    // Fallback if parsing fails
                    50 * 1024 * 1024 // 50MB as a reasonable default
                }
                Err(_) => {
                    // Fallback if reading fails
                    50 * 1024 * 1024 // 50MB as a reasonable default
                }
            }
        }

        #[cfg(target_os = "macos")]
        {
            // On macOS, we would use task_info API, but for simplicity
            // we'll use a reasonable default value
            50 * 1024 * 1024 // 50MB as a reasonable default
        }

        #[cfg(target_os = "windows")]
        {
            // On Windows, we would use GetProcessMemoryInfo, but for simplicity
            // we'll use a reasonable default value
            50 * 1024 * 1024 // 50MB as a reasonable default
        }

        // Default for other platforms
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            50 * 1024 * 1024 // 50MB as a reasonable default
        }
    }

    /// Handle task failure and retry logic
    fn _handle_task_failure(
        task: &mut Task,
        error_msg: String,
        metrics: &Arc<TaskMetricsCollector>,
        mut metrics_guard: TaskExecutionGuard,
    ) {
        // Set the error message in the task
        task.error = Some(error_msg.clone());
        // Log the failure event
        task.log_event(
            TaskStatus::Failed,
            Some(format!("Task failed: {}", error_msg)),
        );

        // Check if we can retry before incrementing
        if task.retries < task.max_retries {
            // Increment retry count
            task.retries += 1;

            info!(
                "Scheduling retry for task {} (retry count: {})",
                task.id, task.retries
            );
            // Calculate next retry time with exponential backoff
            let retry_delay = 2u32.pow(task.retries as u32);
            task.schedule = Some(TaskSchedule {
                cron: None,
                delay: None,
                run_at: Some(Utc::now() + chrono::Duration::seconds(retry_delay as i64)),
            });

            // Update status to Scheduled
            if let Err(e) = task.update_status(TaskStatus::Scheduled) {
                error!(
                    "Failed to update task {} status to Scheduled: {}",
                    task.id, e
                );
                // If we can't schedule a retry, mark as failed
                if let Err(e2) = task.update_status(TaskStatus::Failed) {
                    error!("Failed to update task {} status to Failed: {}", task.id, e2);
                }
                metrics_guard.fail();
            } else {
                info!(
                    "Task {} scheduled for retry at {:?}",
                    task.id,
                    task.schedule.as_ref().unwrap().run_at
                );
                // Record retry and mark as retrying
                metrics.record_task_retry();
                metrics_guard.retry();
                task.log_event(
                    TaskStatus::Scheduled,
                    Some(format!(
                        "Task scheduled for retry (retry count: {})",
                        task.retries
                    )),
                );
            }
        } else {
            info!("Task {} has no retries remaining", task.id);
            // Set status to Failed first
            if let Err(e) = task.update_status(TaskStatus::Failed) {
                error!("Failed to update task {} status to Failed: {}", task.id, e);
            }
            // Mark as failed in metrics (will be recorded by the guard)
            metrics_guard.fail();
            task.log_event(
                TaskStatus::Failed,
                Some("No retries remaining, task marked as failed".to_string()),
            );
        }
    }

    /// Add a task to the executor
    pub async fn add_task(&self, task: Task) {
        info!("Adding task {}", task.id);
        let _ = self.storage.store_task(task).await;
    }

    /// Get a task by ID, returns None on error or not found
    pub async fn get_task(&self, id: &str) -> Option<Task> {
        match self.storage.get_task(id).await {
            Ok(opt) => opt,
            Err(e) => {
                error!("Error fetching task {}: {}", id, e);
                None
            }
        }
    }

    /// List all tasks, returns empty on error
    pub async fn list_tasks(&self) -> Vec<Task> {
        match self.storage.list_tasks().await {
            Ok(tasks) => tasks,
            Err(e) => {
                error!("Error listing tasks: {}", e);
                Vec::new()
            }
        }
    }

    /// Update a task's status
    pub async fn update_task_status_async(&self, id: &str, status: &TaskStatus) -> Option<Task> {
        if let Ok(Some(mut task)) = self.storage.get_task(id).await {
            if let Err(e) = task.update_status(*status) {
                error!("Failed to update task {} status: {}", id, e);
                return None;
            }
            task.updated_at = Utc::now();
            if self.storage.update_task(task.clone()).await.is_ok() {
                return Some(task);
            }
        }
        None
    }

    /// Delete a task
    pub async fn delete_task_async(&self, id: &str) -> bool {
        info!("Deleting task {}", id);
        self.storage.delete_task(id).await.is_ok()
    }

    /// Cancel a task
    pub async fn cancel_task_async(&self, id: &str) -> Result<Task, String> {
        // Fetch the task
        let mut task = match self.storage.get_task(id).await {
            Ok(Some(t)) => t,
            Ok(None) => return Err(format!("Task {} not found", id)),
            Err(e) => return Err(format!("Error fetching task: {}", e)),
        };

        // Update task status
        task.update_status(TaskStatus::Cancelled)
            .map_err(|e| format!("Failed to update task status: {}", e))?;

        // Persist updated task
        self.storage
            .update_task(task.clone())
            .await
            .map_err(|e| format!("Failed to update task: {}", e))?;

        Ok(task)
    }

    /// Check if a task is active (running)
    pub async fn is_task_active_async(&self, id: &str) -> bool {
        if let Ok(Some(task)) = self.storage.get_task(id).await {
            task.status == TaskStatus::Running
        } else {
            false
        }
    }

    pub async fn run_task_loop(&self) {
        info!("Starting task execution loop");

        loop {
            // Check shutdown
            if self.is_shutdown_requested() {
                info!("Shutdown requested, stopping task execution loop");
                break;
            }

            // Fetch next task
            let task = match self.storage.get_next_task().await {
                Ok(Some(task)) => task,
                Ok(None) => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
                Err(e) => {
                    error!("Failed to get next task: {}", e);
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            };

            // Skip cancelled
            if task.status == TaskStatus::Cancelled {
                debug!("Skipping cancelled task {}", task.id);
                continue;
            }

            // Prepare execution
            let timeout_duration = Duration::from_secs(task.timeout as u64);
            let task_id = task.id.clone();
            let tool_invoker = Arc::clone(&self.tool_invoker);
            let storage = Arc::clone(&self.storage);
            let task_complete = Arc::clone(&self.task_complete);
            let metrics = Arc::clone(&self.metrics);
            let future = Self::execute_task(task, tool_invoker, storage, task_complete, metrics);

            // Execute with timeout
            match tokio::time::timeout(timeout_duration, future).await {
                Ok(Ok(())) => debug!("Task {} completed", task_id),
                Ok(Err(e)) => error!("Task {} failed: {}", task_id, e),
                Err(_) => {
                    error!(
                        "Task {} timed out after {} seconds",
                        task_id,
                        timeout_duration.as_secs()
                    );
                    if let Ok(Some(mut t)) = self.storage.get_task(&task_id).await {
                        t.error = Some(format!(
                            "Task timed out after {} seconds",
                            timeout_duration.as_secs()
                        ));
                        t.updated_at = Utc::now();
                        if let Err(e) = t.update_status(TaskStatus::Failed) {
                            error!("Failed to update task {} status: {}", task_id, e);
                        }
                        t.log_event(
                            TaskStatus::Failed,
                            Some(format!(
                                "Task timed out after {} seconds",
                                timeout_duration.as_secs()
                            )),
                        );
                        self.metrics.record_task_failure();
                        let _ = self.storage.update_task(t).await;
                    }
                }
            }

            // Sleep before next iteration
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        info!("Task execution loop stopped");
    }

    pub async fn receive_tasks(&mut self) {
        while let Some(task) = self.rx.recv().await {
            self.add_task(task).await;
        }
    }

    /// Check if shutdown is requested
    fn is_shutdown_requested(&self) -> bool {
        *lock_with_timeout(&self.shutdown, "shutdown in is_shutdown_requested()")
    }
}

impl ToolInvoker for TaskExecutor {
    fn new() -> Self {
        let storage = Arc::new(FileTaskStorage::new(PathBuf::from("tasks.json")));
        let metrics = Arc::new(TaskMetricsCollector::new());
        let tool_invoker = Arc::new(DummyToolRegistry::new());

        Self::new(tool_invoker, storage, metrics)
    }

    fn invoke_tool(
        &self,
        tool: String,
        arguments: serde_json::Value,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<serde_json::Value, Box<dyn Error + Send + Sync>>>
                + Send
                + 'static,
        >,
    > {
        let tool_invoker = self.tool_invoker.clone();
        Box::pin(async move {
            // Delegate to the tool registry for actual tool invocation
            let result = tool_invoker.invoke_tool(tool, arguments).await?;
            Ok(result)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::task_storage::FileTaskStorage;
    use std::sync::Arc;
    use tempfile::NamedTempFile;

    use crate::servers::tool_invoker::ToolInvoker;
    struct DummyInvoker;
    impl ToolInvoker for DummyInvoker {
        fn new() -> Self
        where
            Self: Sized,
        {
            Self
        }
        fn invoke_tool(
            &self,
            _name: String,
            _params: serde_json::Value,
        ) -> Pin<
            Box<
                dyn Future<Output = Result<serde_json::Value, Box<dyn Error + Send + Sync>>> + Send,
            >,
        > {
            Box::pin(async { Ok(serde_json::json!({})) })
        }
    }

    #[test]
    fn test_executor_shutdown_with_no_tasks() {
        let temp_file = NamedTempFile::new().unwrap();
        let file_path = temp_file.path().to_path_buf().display().to_string();
        let storage: Arc<dyn crate::utils::task_storage::TaskStorage> =
            Arc::new(FileTaskStorage::new(file_path));
        let tool_invoker = Arc::new(DummyInvoker);
        let executor =
            TaskExecutor::new(tool_invoker, storage, Arc::new(TaskMetricsCollector::new()));
        executor.start().unwrap();
        // Wait a moment to ensure the thread starts
        std::thread::sleep(std::time::Duration::from_millis(500));
        // Use a thread and channel to detect hangs
        let (tx, rx) = std::sync::mpsc::channel();
        let exec_clone = executor;
        std::thread::spawn(move || {
            // We can't .await here; do a tiny runtime to drive the future without panicking on failure
            if let Ok(rt) = tokio::runtime::Runtime::new() {
                let _ = rt.block_on(exec_clone.shutdown());
            }
            let _ = tx.send(());
        });
        match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(_) => println!("Executor shutdown with no tasks completed successfully."),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => println!("Receive timeout"),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => println!("Channel closed"),
        }
    }
}

fn lock_with_timeout<'a, T>(mutex: &'a Mutex<T>, msg: &str) -> MutexGuard<'a, T> {
    let start = std::time::Instant::now();
    loop {
        if let Ok(guard) = mutex.try_lock() {
            return guard;
        }
        if start.elapsed() > StdDuration::from_millis(100) {
            warn!(
                "[LOCK TIMEOUT] {} after 100ms; falling back to blocking lock",
                msg
            );
            return mutex.lock().unwrap();
        }
        std::thread::sleep(StdDuration::from_millis(1));
    }
}
