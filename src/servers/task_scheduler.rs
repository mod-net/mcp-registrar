use std::error::Error as StdError;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::Error;
use crate::models::task::{Task, TaskSchedule, TaskStatus};
use crate::monitoring::TaskMetricsCollector;
use crate::servers::task_executor::TaskExecutor;
use crate::servers::tool_invoker::ToolInvoker;
use crate::transport::{HandlerResult, McpServer};
use crate::utils::task_storage::{FileTaskStorage, TaskStorage};

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateTaskRequest {
    pub name: String,
    pub params: Value,
    pub schedule: Option<TaskSchedule>,
    pub max_retries: Option<u32>,
    pub timeout: Option<u64>,
    pub frustration_threshold: Option<u32>,
    pub similarity_threshold: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskResponse {
    pub task: Task,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskListResponse {
    pub tasks: Vec<Task>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskEventLogResponse {
    pub id: String,
    pub event_log: Vec<crate::models::task::TaskEvent>,
}

#[derive(Clone)]
pub struct DummyToolRegistry;

impl DummyToolRegistry {
    pub fn new() -> Self {
        Self
    }
}

impl ToolInvoker for DummyToolRegistry {
    fn new() -> Self {
        Self {}
    }

    fn invoke_tool(
        &self,
        _tool: String,
        _arguments: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Value, Box<dyn StdError + Send + Sync>>> + Send>> {
        Box::pin(async {
            Ok(serde_json::json!({
                "status": "success",
                "message": "Dummy tool invocation"
            }))
        })
    }
}

#[derive(Clone)]
pub struct TaskSchedulerServer {
    tool_invoker: Arc<dyn ToolInvoker>,
    storage: Arc<dyn TaskStorage>,
    metrics: Arc<TaskMetricsCollector>,
}

impl TaskSchedulerServer {
    pub fn new(
        tool_invoker: Arc<dyn ToolInvoker>,
        storage: Arc<dyn TaskStorage>,
        metrics: Arc<TaskMetricsCollector>,
    ) -> Self {
        Self {
            tool_invoker,
            storage,
            metrics,
        }
    }

    pub async fn get_task_by_id(&self, task_id: &str) -> Result<Task, Error> {
        self.storage.get_task(task_id).await?.ok_or(Error::NotFound)
    }

    pub async fn get_task(&self, request: CreateTaskRequest) -> Result<Task, Error> {
        // Create a new task with the given request
        let task = Task::new(
            request.name.clone(),
            request.params.clone(),
            request.schedule.clone(),
            request.max_retries,
            request.timeout,
            request.frustration_threshold,
            request.similarity_threshold,
        );

        // Store the task
        self.storage.store_task(task.clone()).await?;

        Ok(task)
    }

    pub async fn cancel_task(&self, task_id: &str) -> Result<Task, Error> {
        if let Some(mut task) = self.storage.get_task(task_id).await? {
            task.set_status(TaskStatus::Cancelled);
            self.storage.store_task(task.clone()).await?;
            self.metrics.record_task_cancellation();
            Ok(task)
        } else {
            Err(Error::NotFound)
        }
    }

    pub async fn list_tasks(&self) -> Result<Vec<Task>, Error> {
        self.storage.list_tasks().await
    }

    pub async fn delete_task(&self, task_id: &str) -> Result<(), Error> {
        if let Some(_task) = self.storage.get_task(task_id).await? {
            self.storage.delete_task(task_id).await?;
            Ok(())
        } else {
            Err(Error::NotFound)
        }
    }

    pub async fn update_task_status(
        &self,
        task_id: &str,
        status: TaskStatus,
    ) -> Result<Task, Error> {
        if let Some(mut task) = self.storage.get_task(task_id).await? {
            task.set_status(status);
            self.storage.store_task(task.clone()).await?;
            Ok(task)
        } else {
            Err(Error::NotFound)
        }
    }
}

impl ToolInvoker for TaskSchedulerServer {
    fn new() -> Self {
        let storage = Arc::new(FileTaskStorage::new(PathBuf::from("tasks.json")));
        let metrics = Arc::new(TaskMetricsCollector::new());
        let tool_invoker = Arc::new(DummyToolRegistry::new());

        Self::new(
            Arc::new(TaskExecutor::new(
                tool_invoker,
                storage.clone(),
                metrics.clone(),
            )),
            storage,
            metrics,
        )
    }

    fn invoke_tool(
        &self,
        tool: String,
        arguments: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Value, Box<dyn StdError + Send + Sync>>> + Send>> {
        let tool_invoker = self.tool_invoker.clone();
        Box::pin(async move { tool_invoker.invoke_tool(tool, arguments).await })
    }
}

#[async_trait]
impl McpServer for TaskSchedulerServer {
    async fn handle(&self, name: &str, params: Value) -> HandlerResult {
        match name {
            "CreateTask" => {
                let request: CreateTaskRequest = serde_json::from_value(params)?;
                let task = self.get_task(request).await?;
                Ok(serde_json::to_value(TaskResponse { task })?)
            }
            "GetTask" => {
                let id: String = serde_json::from_value(params)?;
                let task = self.get_task_by_id(&id).await?;
                Ok(serde_json::to_value(TaskResponse { task })?)
            }
            "ListTasks" => {
                let tasks = self.list_tasks().await?;
                Ok(serde_json::to_value(tasks)?)
            }
            "CancelTask" => {
                let id: String = serde_json::from_value(params)?;

                let mut task = self.get_task_by_id(&id).await?;

                task.update_status(TaskStatus::Cancelled)?;

                self.storage.update_task(task.clone()).await?;

                Ok(serde_json::to_value(TaskResponse { task })?)
            }
            "DeleteTask" => {
                let id = params["id"].as_str().ok_or("Missing task id")?;
                self.delete_task(id).await?;
                Ok(serde_json::json!({ "success": true }))
            }
            "UpdateTaskStatus" => {
                let id = params["id"].as_str().ok_or("Missing task id")?;
                let status_str = params["status"].as_str().ok_or("Missing status")?;
                let status = match status_str {
                    "pending" => TaskStatus::Pending,
                    "running" => TaskStatus::Running,
                    "completed" => TaskStatus::Completed,
                    "failed" => TaskStatus::Failed,
                    "cancelled" => TaskStatus::Cancelled,
                    "scheduled" => TaskStatus::Scheduled,
                    _ => return Err(format!("Invalid status: {}", status_str).into()),
                };
                let task = self.update_task_status(id, status).await?;
                Ok(serde_json::to_value(TaskResponse { task })?)
            }
            "GetTaskEventLog" => {
                let id = params["id"].as_str().ok_or("Missing task id")?;
                let task_opt = self.get_task_by_id(id).await?;
                let task = task_opt.ok_or_else(|| format!("Task not found: {}", id))?;
                Ok(serde_json::to_value(TaskEventLogResponse {
                    id: task.id.clone(),
                    event_log: task.event_log.clone(),
                })?)
            }
            _ => Err(format!("Unknown method: {}", name).into()),
        }
    }
}
