# /// script
# requires-python = ">=3.12"
# dependencies = [
#     "graphviz",
#     "matplotlib",
# ]
# ///
import matplotlib.pyplot as plt
import graphviz

# Architecture Overview Diagram
architecture_dot = """
digraph architecture {
    rankdir=LR;
    node [shape=box style=filled fillcolor=lightgrey];

    ToolRegistry [label="ToolRegistry\n(MCP Server)"];
    ResourceRegistry [label="ResourceRegistry\n(MCP Server)"];
    PromptRegistry [label="PromptRegistry\n(MCP Server)"];
    McpRegistrar [label="McpRegistrar\n(MCP Server)"];
    TaskScheduler [label="TaskScheduler\n(MCP Server)"];

    ToolRegistry -> McpRegistrar [dir=both];
    ResourceRegistry -> McpRegistrar [dir=both];
    PromptRegistry -> McpRegistrar [dir=both];
    TaskScheduler -> ToolRegistry [label="InvokeTool"];
    TaskScheduler -> McpRegistrar [label="Discovery"];

    McpRegistrar -> TaskScheduler [label="Registry Info"];
}
"""

# Task Lifecycle Diagram
lifecycle_dot = """
digraph lifecycle {
    rankdir=TB;
    node [shape=box style=filled fillcolor=lightblue];

    Created [label="Task Created"];
    Scheduled [label="Scheduled or Immediate Execution"];
    Resolved [label="Resolved Tool via ToolRegistry"];
    Execution [label="Execution in Isolated Context"];
    Success [label="Status: Completed"];
    Failure [label="Status: Failed or Retry"];

    Created -> Scheduled -> Resolved -> Execution;
    Execution -> Success;
    Execution -> Failure;
}
"""

# Generate diagrams
architecture_graph = graphviz.Source(architecture_dot)
lifecycle_graph = graphviz.Source(lifecycle_dot)

architecture_graph.render('public/images/mcp_architecture', format='png', cleanup=False)
lifecycle_graph.render('public/images/task_lifecycle', format='png', cleanup=False)

"public/images/mcp_architecture.png", "public/images/task_lifecycle.png"
