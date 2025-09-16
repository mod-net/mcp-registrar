use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "registry-scheduler")]
#[command(about = "MCP Registry Scheduler - A modular network of composable MCP servers")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start the MCP Registrar server
    #[command(name = "start-registrar")]
    StartRegistrar,

    /// Start the Tool Registry server
    #[command(name = "start-tool-registry")]
    StartToolRegistry,

    /// Start the Resource Registry server
    #[command(name = "start-resource-registry")]
    StartResourceRegistry,

    /// Start the Prompt Registry server
    #[command(name = "start-prompt-registry")]
    StartPromptRegistry,

    /// Start the Task Scheduler server
    #[command(name = "start-task-scheduler")]
    StartTaskScheduler,

    /// Register a tool
    #[command(name = "register-tool")]
    RegisterTool,

    /// List registered tools
    #[command(name = "list-tools")]
    ListTools,

    /// Execute a registered tool
    #[command(name = "execute-tool")]
    ExecuteTool {
        /// ID of the tool to execute
        #[arg(short, long)]
        tool_id: String,

        /// JSON parameters for the tool
        #[arg(short, long)]
        parameters: String,
    },
}

pub fn parse_args() -> Command {
    let cli = Cli::parse();
    cli.command.unwrap_or_else(|| {
        // If no command is provided, print help
        use clap::CommandFactory;
        let mut cmd = Cli::command();
        cmd.print_help().unwrap();
        std::process::exit(0);
    })
}
