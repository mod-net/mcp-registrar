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

    /// Scaffold a new module under tools/<name>
    #[command(name = "scaffold-module")]
    ScaffoldModule {
        /// Module name (used for directory and script names)
        #[arg(long)]
        name: String,

        /// Runtime type: python-uv-script | binary
        #[arg(long)]
        runtime: String,

        /// Version (semver)
        #[arg(long, default_value = "0.1.0")]
        version: String,

        /// Description
        #[arg(long, default_value = "")]
        description: String,

        /// Categories (comma-separated)
        #[arg(long, default_value = "")]
        categories: String,

        /// Python deps for PEP 723 (comma or space separated), only for python-uv-script
        #[arg(long, default_value = "")]
        deps: String,

        /// Extra uv args (space separated), only for python-uv-script
        #[arg(long, default_value = "")]
        uv_args: String,

        /// Command path for binary runtime
        #[arg(long, default_value = "")]
        command: String,

        /// Default args for binary runtime (space separated)
        #[arg(long, default_value = "")]
        args: String,

        /// Generate an adapter wrapper for binary runtime
        #[arg(long, default_value_t = false)]
        adapter: bool,

        /// Adapter implementation language (currently only 'python')
        #[arg(long, default_value = "python")]
        adapter_lang: String,

        /// Adapter output mode: auto|text|json (auto attempts JSON parse first)
        #[arg(long, default_value = "auto")]
        adapter_mode: String,

        /// Adapter argument style for mapping JSON args to CLI: gnu|posix
        #[arg(long, default_value = "gnu")]
        adapter_arg_style: String,
    },

    /// Run the tool registry as a one-shot tool (stdin JSON -> stdout JSON)
    #[command(name = "registry-tool")]
    RegistryTool,
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
