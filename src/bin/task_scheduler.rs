use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;

use mcp_registrar::servers::task_scheduler::DummyToolRegistry;
use mcp_registrar::servers::task_scheduler::TaskSchedulerServer;
use mcp_registrar::transport::stdio_transport::{StdioTransportServer, TransportServer};
use mcp_registrar::utils::task_storage::{FileTaskStorage, TaskStorage};
use mcp_registrar::TaskExecutor;
use mcp_registrar::TaskMetricsCollector;

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
    let storage: Arc<dyn TaskStorage> = Arc::new(FileTaskStorage::new(PathBuf::from("tasks.json")));

    let tool_invoker = Arc::new(DummyToolRegistry::new());

    let scheduler = TaskSchedulerServer::new(
        Arc::new(TaskExecutor::new(
            tool_invoker,
            storage.clone(),
            Arc::new(TaskMetricsCollector::new()),
        )),
        storage,
        Arc::new(TaskMetricsCollector::new()),
    );

    // Initialize stdio transport with the server
    tracing::info!("Starting Task Scheduler server with stdio transport");
    let transport = StdioTransportServer::new(scheduler);

    // Start the server
    transport.serve().await?;

    Ok(())
}
