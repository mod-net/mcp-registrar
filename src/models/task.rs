use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::hash::Hash;
use std::sync::OnceLock;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    Scheduled,
}

impl TaskStatus {
    /// Get the valid next states for this status
    fn valid_next_states(&self) -> &'static HashSet<TaskStatus> {
        static PENDING_NEXT: OnceLock<HashSet<TaskStatus>> = OnceLock::new();
        static RUNNING_NEXT: OnceLock<HashSet<TaskStatus>> = OnceLock::new();
        static FAILED_NEXT: OnceLock<HashSet<TaskStatus>> = OnceLock::new();
        static SCHEDULED_NEXT: OnceLock<HashSet<TaskStatus>> = OnceLock::new();
        static COMPLETED_NEXT: OnceLock<HashSet<TaskStatus>> = OnceLock::new();
        static CANCELLED_NEXT: OnceLock<HashSet<TaskStatus>> = OnceLock::new();

        match self {
            TaskStatus::Pending => PENDING_NEXT.get_or_init(|| {
                let mut set = HashSet::new();
                set.insert(TaskStatus::Running);
                set.insert(TaskStatus::Scheduled);
                set.insert(TaskStatus::Cancelled);
                set
            }),
            TaskStatus::Running => RUNNING_NEXT.get_or_init(|| {
                let mut set = HashSet::new();
                set.insert(TaskStatus::Completed);
                set.insert(TaskStatus::Failed);
                set.insert(TaskStatus::Cancelled);
                set.insert(TaskStatus::Scheduled); // Allow direct retry scheduling
                set
            }),
            TaskStatus::Failed => FAILED_NEXT.get_or_init(|| {
                let mut set = HashSet::new();
                set.insert(TaskStatus::Scheduled); // For retries
                set
            }),
            TaskStatus::Scheduled => SCHEDULED_NEXT.get_or_init(|| {
                let mut set = HashSet::new();
                set.insert(TaskStatus::Running);
                set.insert(TaskStatus::Cancelled);
                set
            }),
            TaskStatus::Completed => COMPLETED_NEXT.get_or_init(|| HashSet::new()),
            TaskStatus::Cancelled => CANCELLED_NEXT.get_or_init(|| HashSet::new()),
        }
    }

    /// Check if a transition to the target status is valid
    pub fn can_transition_to(&self, target: TaskStatus) -> bool {
        self.valid_next_states().contains(&target)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TaskSchedule {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cron: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TaskResponseCache {
    pub response: String,
    pub timestamp: DateTime<Utc>,
    pub similarity_score: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResourceLimits {
    pub memory_bytes: u64,
    pub cpu_time_ms: u64,
    pub max_concurrent: u32,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            memory_bytes: 1024 * 1024 * 1024, // 1GB
            cpu_time_ms: 60000,               // 1 minute
            max_concurrent: 10,               // 10 concurrent tasks
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TaskEvent {
    pub timestamp: DateTime<Utc>,
    pub status: TaskStatus,
    pub message: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Task {
    pub id: String,
    pub tool: String,
    pub arguments: serde_json::Value,
    pub status: TaskStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule: Option<TaskSchedule>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub retries: u32,
    pub max_retries: u32,
    pub timeout: u64,
    pub frustration: u32,
    pub frustration_threshold: u32,
    pub similarity_threshold: f32,
    #[serde(skip_serializing_if = "Vec::is_empty", default = "Vec::new")]
    pub response_cache: Vec<TaskResponseCache>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_limits: Option<ResourceLimits>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_usage: Option<ResourceLimits>,
    #[serde(skip_serializing_if = "Vec::is_empty", default = "Vec::new")]
    pub event_log: Vec<TaskEvent>,
}

impl Task {
    pub fn new(
        tool: String,
        arguments: serde_json::Value,
        schedule: Option<TaskSchedule>,
        max_retries: Option<u32>,
        timeout: Option<u64>,
        frustration_threshold: Option<u32>,
        similarity_threshold: Option<f32>,
    ) -> Self {
        let now = Utc::now();
        let initial_status = TaskStatus::Pending;
        Self {
            id: Uuid::new_v4().to_string(),
            tool,
            arguments,
            status: initial_status,
            created_at: now,
            updated_at: now,
            schedule,
            result: None,
            error: None,
            retries: 0,
            max_retries: max_retries.unwrap_or(3),
            timeout: timeout.unwrap_or(60),
            frustration: 0,
            frustration_threshold: frustration_threshold.unwrap_or(3),
            similarity_threshold: similarity_threshold.unwrap_or(0.85),
            response_cache: Vec::new(),
            resource_limits: Some(ResourceLimits::default()),
            resource_usage: None,
            event_log: vec![TaskEvent {
                timestamp: now,
                status: initial_status,
                message: Some("Task created".to_string()),
            }],
        }
    }

    pub fn is_ready_to_run(&self) -> bool {
        // Allow both Pending and Scheduled tasks to run
        if self.status != TaskStatus::Pending && self.status != TaskStatus::Scheduled {
            return false;
        }

        if let Some(schedule) = &self.schedule {
            if let Some(run_at) = schedule.run_at {
                return run_at <= Utc::now();
            }
        }

        true
    }

    pub fn can_retry(&self) -> bool {
        self.status == TaskStatus::Failed && self.retries < self.max_retries
    }

    /// Update the task status with validation
    pub fn update_status(&mut self, new_status: TaskStatus) -> Result<(), String> {
        if !self.status.can_transition_to(new_status) {
            return Err(format!(
                "Invalid status transition from {:?} to {:?}",
                self.status, new_status
            ));
        }

        self.status = new_status;
        self.updated_at = Utc::now();
        self.log_event(
            new_status,
            Some(format!("Status changed to {:?}", new_status)),
        );
        Ok(())
    }

    /// Add a response to the cache, maintaining only the last 5 responses
    pub fn cache_response(&mut self, response: String) {
        let cache_entry = TaskResponseCache {
            response,
            timestamp: Utc::now(),
            similarity_score: None,
        };

        self.response_cache.push(cache_entry);
        if self.response_cache.len() > 5 {
            self.response_cache.remove(0);
        }
    }

    /// Check if we're stuck in a loop based on semantic similarity
    pub fn is_stuck_in_loop(&self) -> bool {
        if self.response_cache.len() < 2 {
            return false;
        }

        let similar_responses = self
            .response_cache
            .iter()
            .filter(|entry| entry.similarity_score.unwrap_or(0.0) > self.similarity_threshold)
            .count();

        similar_responses >= 2
    }

    /// Check if we should trigger the frustration interceptor
    pub fn should_intercept(&self) -> bool {
        self.frustration >= self.frustration_threshold || self.is_stuck_in_loop()
    }

    pub fn log_event(&mut self, status: TaskStatus, message: Option<String>) {
        self.event_log.push(TaskEvent {
            timestamp: Utc::now(),
            status,
            message,
        });
    }

    pub fn set_status(&mut self, status: TaskStatus) {
        self.status = status;
        self.updated_at = Utc::now();
    }

    pub fn ok_or_else<E, F>(self, _error_fn: F) -> Result<Task, E>
    where
        F: FnOnce() -> E,
    {
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_creation() {
        let tool = "test-tool".to_string();
        let arguments = serde_json::json!({ "param1": "value1" });

        let task = Task::new(
            tool.clone(),
            arguments.clone(),
            None,
            Some(3),
            Some(60),
            None,
            None,
        );

        assert_eq!(task.tool, tool);
        assert_eq!(task.arguments, arguments);
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.retries, 0);
        assert_eq!(task.max_retries, 3);
        assert_eq!(task.timeout, 60);
        assert_eq!(task.frustration, 0);
        assert!(task.schedule.is_none());
        assert!(task.result.is_none());
        assert!(task.error.is_none());
    }

    #[test]
    fn test_task_is_ready_to_run() {
        // Create a task with a future run_at time
        let mut task = Task::new(
            "test-tool".to_string(),
            serde_json::json!({ "param1": "value1" }),
            Some(TaskSchedule {
                cron: None,
                delay: None,
                run_at: Some(Utc::now() + chrono::Duration::hours(1)),
            }),
            Some(0),
            Some(60),
            None,
            None,
        );

        // Task should not be ready to run (future run_at time)
        assert!(
            !task.is_ready_to_run(),
            "Task with future run_at time should not be ready to run"
        );

        // Set run_at to the past
        task.schedule = Some(TaskSchedule {
            cron: None,
            delay: None,
            run_at: Some(Utc::now() - chrono::Duration::hours(1)),
        });

        // Task should be ready to run (past run_at time)
        assert!(
            task.is_ready_to_run(),
            "Task with past run_at time should be ready to run"
        );

        // Set status to Running
        task.status = TaskStatus::Running;

        // Task should not be ready to run (already running)
        assert!(
            !task.is_ready_to_run(),
            "Running task should not be ready to run"
        );

        // Set status to Completed
        task.status = TaskStatus::Completed;

        // Task should not be ready to run (already completed)
        assert!(
            !task.is_ready_to_run(),
            "Completed task should not be ready to run"
        );

        // Set status to Failed
        task.status = TaskStatus::Failed;

        // Task should not be ready to run (already failed)
        assert!(
            !task.is_ready_to_run(),
            "Failed task should not be ready to run"
        );

        // Set status to Pending
        task.status = TaskStatus::Pending;

        // Task should be ready to run (Pending status)
        assert!(
            task.is_ready_to_run(),
            "Pending task should be ready to run"
        );

        // Set status to Scheduled
        task.status = TaskStatus::Scheduled;

        // Task should be ready to run (Scheduled status)
        assert!(
            task.is_ready_to_run(),
            "Scheduled task should be ready to run"
        );
    }

    #[test]
    fn test_task_can_retry() {
        // Cannot retry because not failed
        let task = Task {
            id: Uuid::new_v4().to_string(),
            tool: "test-tool".to_string(),
            arguments: serde_json::json!({}),
            status: TaskStatus::Pending,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            schedule: None,
            result: None,
            error: None,
            retries: 0,
            max_retries: 3,
            timeout: 60,
            frustration: 0,
            frustration_threshold: 3,
            similarity_threshold: 0.85,
            response_cache: Vec::new(),
            resource_limits: None,
            resource_usage: None,
            event_log: Vec::new(),
        };
        assert!(!task.can_retry());

        // Can retry because failed and retries < max_retries
        let mut task = task.clone();
        task.status = TaskStatus::Failed;
        assert!(task.can_retry());

        // Cannot retry because retries = max_retries
        let mut task = task.clone();
        task.retries = 3;
        assert!(!task.can_retry());
    }

    #[test]
    fn test_task_serialization() {
        let task = Task::new(
            "test-tool".to_string(),
            serde_json::json!({ "param1": "value1" }),
            Some(TaskSchedule {
                cron: Some("* * * * *".to_string()),
                delay: None,
                run_at: None,
            }),
            Some(3),
            Some(60),
            None,
            None,
        );

        let serialized = serde_json::to_string(&task).unwrap();
        let deserialized: Task = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.id, task.id);
        assert_eq!(deserialized.tool, task.tool);
        assert_eq!(deserialized.status, task.status);
        assert_eq!(deserialized.max_retries, task.max_retries);

        if let Some(schedule) = deserialized.schedule {
            assert_eq!(schedule.cron, Some("* * * * *".to_string()));
        } else {
            panic!("Schedule was not deserialized correctly");
        }
    }

    #[test]
    fn test_status_transitions() {
        let mut task = Task::new(
            "test-tool".to_string(),
            serde_json::json!({}),
            None,
            Some(3),
            Some(60),
            None,
            None,
        );

        // Test valid transitions
        assert_eq!(task.status, TaskStatus::Pending);

        // Pending -> Running
        assert!(task.update_status(TaskStatus::Running).is_ok());
        assert_eq!(task.status, TaskStatus::Running);

        // Running -> Completed
        assert!(task.update_status(TaskStatus::Completed).is_ok());
        assert_eq!(task.status, TaskStatus::Completed);

        // Test invalid transitions
        let mut task = Task::new(
            "test-tool".to_string(),
            serde_json::json!({}),
            None,
            Some(3),
            Some(60),
            None,
            None,
        );

        // Pending -> Completed (invalid)
        assert!(task.update_status(TaskStatus::Completed).is_err());
        assert_eq!(task.status, TaskStatus::Pending);

        // Test cancellation transitions
        // Pending -> Cancelled
        assert!(task.update_status(TaskStatus::Cancelled).is_ok());
        assert_eq!(task.status, TaskStatus::Cancelled);

        // Test retry transitions
        let mut task = Task::new(
            "test-tool".to_string(),
            serde_json::json!({}),
            None,
            Some(3),
            Some(60),
            None,
            None,
        );

        // Pending -> Running -> Failed -> Scheduled
        assert!(task.update_status(TaskStatus::Running).is_ok());
        assert!(task.update_status(TaskStatus::Failed).is_ok());
        assert!(task.update_status(TaskStatus::Scheduled).is_ok());
        assert_eq!(task.status, TaskStatus::Scheduled);
    }

    #[test]
    fn test_task_status_update() {
        let mut task = Task::new(
            "test-tool".to_string(),
            serde_json::json!({}),
            None,
            Some(3),
            Some(60),
            None,
            None,
        );

        // Test valid transition
        assert_eq!(task.status, TaskStatus::Pending);
        assert!(task.update_status(TaskStatus::Running).is_ok());
        assert_eq!(task.status, TaskStatus::Running);

        // Test invalid transition
        assert!(task.update_status(TaskStatus::Pending).is_err());
        assert_eq!(task.status, TaskStatus::Running);

        // Test transition to completion
        assert!(task.update_status(TaskStatus::Completed).is_ok());
        assert_eq!(task.status, TaskStatus::Completed);

        // Test no transitions allowed from completed
        assert!(task.update_status(TaskStatus::Running).is_err());
        assert_eq!(task.status, TaskStatus::Completed);
    }
}
