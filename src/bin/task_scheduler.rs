use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;

use registry_scheduler::TaskMetricsCollector;
use registry_scheduler::servers::task_scheduler::DummyToolRegistry;
use registry_scheduler::TaskExecutor;
use registry_scheduler::utils::task_storage::{TaskStorage, FileTaskStorage};
use registry_scheduler::servers::task_scheduler::TaskSchedulerServer;
use registry_scheduler::transport::stdio_transport::{StdioTransportServer, TransportServer};

/// Task Scheduler Server CLI
#[derive(Debug, Parser)]
#[command(name = "task-scheduler")]
#[command(about = "Task Scheduler Server - MCP task scheduling and execution")]
enum Cli {
    StartTaskScheduler,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Parse command line arguments
    let _args = Cli::parse();

    // Create a new FileTaskStorage instance
    let storage: Arc<dyn TaskStorage> = 
        Arc::new(FileTaskStorage::new(PathBuf::from("tasks.json")));

    let tool_invoker = Arc::new(DummyToolRegistry::new());

    let scheduler = TaskSchedulerServer::new(
        Arc::new(TaskExecutor::new(
            tool_invoker, 
            storage.clone(), 
            Arc::new(TaskMetricsCollector::new())
        )), 
        storage, 
        Arc::new(TaskMetricsCollector::new())
    );

    // Initialize stdio transport with the server
    tracing::info!("Starting Task Scheduler server with stdio transport");
    let transport = StdioTransportServer::new(scheduler);

    // Start the server
    transport.serve().await?;

    Ok(())
}
