use crate::error::Error;
use crate::models::task::{Task, TaskStatus};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[async_trait]
pub trait TaskStorage: Send + Sync {
    async fn store_task(&self, task: Task) -> Result<(), Error>;
    /// Retrieve a task by ID; returns Ok(Some(task)) or Ok(None) if not found
    async fn get_task(&self, task_id: &str) -> Result<Option<Task>, Error>;
    async fn list_tasks(&self) -> Result<Vec<Task>, Error>;
    async fn update_task(&self, task: Task) -> Result<(), Error>;
    async fn delete_task(&self, task_id: &str) -> Result<(), Error>;
    /// Retrieve the next available task (e.g., for execution loop)
    async fn get_next_task(&self) -> Result<Option<Task>, Error>;
}

pub struct FileTaskStorage {
    storage_path: String,
    tasks: Arc<Mutex<HashMap<String, Task>>>,
}

impl FileTaskStorage {
    pub fn new(storage_path: impl AsRef<Path>) -> Self {
        Self {
            storage_path: storage_path.as_ref().to_path_buf().display().to_string(),
            tasks: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    pub fn get_storage_path(&self) -> &str {
        &self.storage_path
    }
}

#[async_trait]
impl TaskStorage for FileTaskStorage {
    async fn store_task(&self, task: Task) -> Result<(), Error> {
        let mut m = self.tasks.lock().unwrap();
        m.insert(task.id.clone(), task);
        Ok(())
    }

    async fn get_task(&self, task_id: &str) -> Result<Option<Task>, Error> {
        let m = self.tasks.lock().unwrap();
        Ok(m.get(task_id).cloned())
    }

    async fn list_tasks(&self) -> Result<Vec<Task>, Error> {
        let m = self.tasks.lock().unwrap();
        Ok(m.values().cloned().collect())
    }

    async fn update_task(&self, task: Task) -> Result<(), Error> {
        let mut m = self.tasks.lock().unwrap();
        if m.contains_key(&task.id) {
            m.insert(task.id.clone(), task);
            Ok(())
        } else {
            Err(Error::NotFound)
        }
    }

    async fn delete_task(&self, task_id: &str) -> Result<(), Error> {
        let mut m = self.tasks.lock().unwrap();
        m.remove(task_id);
        Ok(())
    }

    async fn get_next_task(&self) -> Result<Option<Task>, Error> {
        let m = self.tasks.lock().unwrap();
        let next = m
            .values()
            .filter(|t| t.status == TaskStatus::Pending)
            .cloned()
            .next();
        Ok(next)
    }
}

// For convenience in passing around task storage implementations
pub type TaskStorageRef = Arc<dyn TaskStorage>;
