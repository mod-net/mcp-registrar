### Tool Runtimes and Execution Contract

Tools are executed by the Tool Registry via process-based runtimes. The registry supports:

- process: run any executable with fixed command/args.
- python-uv-script: run a single-file Python script via `uv run`, using PEP 723 metadata to declare dependencies.
- binary: run a native binary.

All runtimes use the same stdin/stdout contract:

- Input (stdin): a single JSON line
  - `{ "arguments": <params> }`
- Output (stdout): a single JSON line. Either
  - Direct MCP content: `{ "content": [ ... ], "isError": bool }`, or
  - Any JSON value, which the MCP gateway wraps as `{ content: [{ type: "json", json: <value> }], isError: false }`.

JSON Schema

- tools/list returns each tool with `inputSchema` populated from the registry’s parameter schema.
- Return schemas are used internally for validation; the MCP gateway returns content arrays, not output schemas.

# Model Context Protocol (MCP)

MCP is a protocol that allows the model to access tools and resources through a RPC interface. It is implemented as a server that will provide their service to the mcp client through RPC Commands.

There is a `rmcp` MCP library that can be used if we like in `/home/bakobi/repos/mcp/rust-sdk` but there are a lot of sharpe edges with their implementation and so far I have prefered to just implement my own RPC client and server for the tools prefering StdIO transports when possible but SSE and WebSockets are also available.

### Tools:

MCP Tools are tools that are provided to the model through the model context protocol(MCP) and are implemented as servers that will provide their service to the mcp client through RPC Commands.

```json
# tool
{
  "name": "string",          // Unique identifier for the tool
  "description": "string",  // Human-readable description
  "inputSchema": {         // JSON Schema for the tool's parameters
    "type": "object",
    "properties": { ... }  // Tool-specific parameters
  },
  "annotations": {        // Optional hints about tool behavior
    "title": "string",      // Human-readable title for the tool
    "readOnlyHint": false,    // If true, the tool does not modify its environment
    "destructiveHint": false, // If true, the tool may perform destructive updates
    "idempotentHint": false,  // If true, repeated calls with same args have no additional effect
    "openWorldHint": false,   // If true, tool interacts with external entities
  }
}
```

```json
# tools/call - Call a tool
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "<TOOL_ID>",
    "arguments": { "key": "value" }
  },
  "id": "<MSG_ID>"
}
```

```json
# tools/list - List available tools
{
    "jsonrpc": "2.0",
    "method": "tools/list",
    "params": {},
    "id": "<MSG_ID>"
}
```

```json
# result - Result of a tool call (MCP-native)
{
  "jsonrpc": "2.0",
  "result": {
    "content": [
      { "type": "text", "text": "..." }
      // or { "type": "json", "json": { /* payload */ } }
    ],
    "isError": false
  },
  "id": "<MSG_ID>"
}
```

```json
# error - Error response
{
    "jsonrpc": "2.0",
    "error": {
        "code": "number",
        "message": "string",
        "data": "value"
    },
    "id": "<MSG_ID>"
}
```

```json
# notification - One way message that expects not response
{
    "method": "initialize",
    "params": {
        "protocol_version": "1.0",
        "capabilities": {
            "tools": true,
            "notifications": true,
        }
    }
}
```

### Connection Lifecycle

1. Initialize connection request with protocol version and capabilities
2. Server responds with protocol version and capabilities
3. Client sends a initilialized notification.
4. Normal communication exchanging requests and responses.
5. Client sends a closed notification.
6. Server closes connection.

### Resources

Resources are files and data provided to the model through a MCP server like databases or the project filesystem.

Resources are selected and injected into the model context by the orchestrator protocol writing them into FSM Protocol `Available Resources` based on the `context` they will be working in. Almost all protocols will get the file system as a resource for the workspace root.

**Types of resources**:

- Text Resources
  - Source code
  - Configuration files
  - Log files
  - Plain text
- Binary Resources
  - Images
  - Videos
  - Audio
  - PDFs
  - Other non-text formats

````json
# resource
{
    "uri": "file:///path/to/resource",
    "name": "resource_name",
    "description": "Optional description of the resource",
    "mimeType": "type of data in the resource",
}

```json
# resources/template - Template to modify dynamic resources
{
    "uriTemplate": "<RFC_6578_Template>",
    "name": "resource_name",
    "description": "Optional description of the resource",
    "mimeType": "type of data in the resource",
}

```json
# resources/list
{
    "jsonrpc": "2.0",
    "method": "resources/list",
    "params": {},
    "id": "<MSG_ID>"
}
````

````json
# resources/list/result
{
    "jsonrpc": "2.0",
    "result": [
        {
            "uri": "file:///path/to/resource",
            "name": "resource_name",
            "description": "Optional description of the resource",
            "mimeType": "type of data in the resource",
        }
    ],
    "id": "<MSG_ID>"
}

```json
# resources/read
{
    "jsonrpc": "2.0",
    "method": "resources/read",
    "params": {
        "uri": "file:///path/to/resource",
        "name": "resource_name",
    },
    "id": "<MSG_ID>"
}
````

```json
# resources/read/result
{
    "jsonrpc": "2.0",
    "result": {
        "uri": "file:///path/to/resource",
        "mimeType": "type of data in the resource",
        "text": "optional text content of the resource",
        "blob": "optional binary content of the resource",
    },
    "id": "<MSG_ID>"
}
```

```json
# resources/write
{
    "jsonrpc": "2.0",
    "method": "resources/write",
    "params": {
        "uri": "file:///path/to/resource",
        "name": "resource_name",
        "mimeType": "type of data in the resource",
        "text": "optional text content of the resource",
        "blob": "optional binary content of the resource",
    },
    "id": "<MSG_ID>"
}
```

```json
# resources/write/result
{
    "jsonrpc": "2.0",
    "result": {
        "uri": "file:///path/to/resource",
        "name": "resource_name",
        "code": 200,
        "message": "Resource written successfully",
    },
    "id": "<MSG_ID>"
}
```

```json
# resources/delete
{
    "jsonrpc": "2.0",
    "method": "resources/delete",
    "params": {
        "uri": "file:///path/to/resource",
        "name": "resource_name",
    },
    "id": "<MSG_ID>"
}
```

```json
# resources/delete/result
{
    "jsonrpc": "2.0",
    "result": {
        "uri": "file:///path/to/resource",
        "name": "resource_name",
        "code": 200,
        "message": "Resource deleted successfully",
    },
    "id": "<MSG_ID>"
}
```

### Subscriptions and Notifications

Subscriptions allow the model to subscribe to resources and receive notifications when they are updated.

```json
# resources/subscribe
{
    "jsonrpc": "2.0",
    "method": "resources/subscribe",
    "params": {
        "uri": "file:///path/to/resource",
        "name": "resource_name",
    },
    "id": "<MSG_ID>"
}
```

```json
# resources/subscribe/result
{
    "jsonrpc": "2.0",
    "result": {
        "uri": "file:///path/to/resource",
        "name": "resource_name",
        "code": 200,
        "message": "Resource subscribed successfully",
    },
    "id": "<MSG_ID>"
}
```

```json
# notification/resources/updated
{
    "jsonrpc": "2.0",
    "result": {
        "uri": "file:///path/to/resource",
        "mimeType": "type of data in the resource",
        "text": "optional text content of the resource",
        "blob": "optional binary content of the resource",
    },
    "id": "<MSG_ID>"
}
```

```json
# notification/resources/unsubscribed
{
    "jsonrpc": "2.0",
    "result": {
        "uri": "file:///path/to/resource",
        "name": "resource_name",
    },
    "id": "<MSG_ID>"
}
```

```json
# notification/resources/unsubscribed/result
{
    "jsonrpc": "2.0",
    "result": {
        "uri": "file:///path/to/resource",
        "name": "resource_name",
        "code": 200,
        "message": "Resource unsubscribed successfully",
    },
    "id": "<MSG_ID>"
}
```

### Prompts

Prompts are preconfigured prompt template with optional dynamic variables that the model can use to generate specific requests with custom context.

```json
# prompt
{
    "name": "prompt name",
    "description": "description of the prompt",
    "arguments": [
        {
            "name": "argument name",
            "description": "description of the dynamic arguement",
            "required": "bool indicating if the argument is required",
        }
    ]
}
```

When the orchestrator requests a prompt it should be assumed that it is a request for inference. The server should fill the prompt arguments with values passed and then generate inference with the cusomized prompt.

We will want to save and collect these for future use.

```json
# prompt/list
{

    "jsonrpc": "2.0",
    "method": "prompt/list",
    "params": {},
    "id": "<MSG_ID>"
}
```

```json
# prompt/list/result
{
    "jsonrpc": "2.0",
    "result": [
        {
            "name": "prompt name",
            "description": "description of the prompt",
            "arguments": [
                {
                    "name": "argument name",
                    "description": "description of the dynamic arguement",
                    "required": "bool indicating if the argument is required",
                }
            ]
        }
    ],
    "id": "<MSG_ID>"
}
```

```json
# prompt/call - this is marked as prompts/get in the model context protocol library but i dont like that it differs from the previous conventions so I'm modifying it for our uses. We almost never will request prompts from third party servers regardless.
{
    "jsonrpc": "2.0",
    "method": "prompt/call",
    "params": {
        "prompt_name": "<PROMPT_NAME>",
        "arguments": {
            "<ARGUMENT_NAME>": "<ARGUMENT_VALUE>"
        }
    },
    "id": "<MSG_ID>"
}
```

```json
# prompt/call/result
{
    "jsonrpc": "2.0",
    "result": {
        "content": "<INFERENCED_CONTENT>",
        "model": "<MODEL_NAME>",
        "usage": {
            "prompt_tokens": <PROMPT_TOKENS>,
            "completion_tokens": <COMPLETION_TOKENS>,
            "total_tokens": <TOTAL_TOKENS>
        }
    },
    "id": "<MSG_ID>"
}
```

We should craft a context prompt that we can dynamically fill with the compiled context document as well as a conversation context prompt that will structure the entire available context including conversation history with a token limit dropping the oldest messages as we go. We should archive those messages rather than just delete them for future training.

### OpenAI API integration

We can attach these tools to the OpenAI API requests and response objects to provide a more complete context to the model. Using the MCP servers to attach the tool results as messages to the conversation history.

```json
{
  "messages": [
    {
      "role": "system",
      "content": "compiled context document"
    },
    {
      "role": "user",
      "content": "development request"
    },
    {
      "role": "assistant",
      "content": "assistance response"
    }
  ]
}
```

Ultimately we will want to convert all response and request types to these messages as a context history for the model to track its work. Every time we swap contexts we will swap out this conversation history and save it. Context that exceed the context limit will be archived for future training and retrival if needed"

```json
{
  "messages": [
    {
      "role": "system",
      "content": "compiled context document"
    }
  ],
  "tool_calls": [
    {
      "resource_call": {
        "jsonrpc": "2.0",
        "method": "resources/read",
        "params": {
          "uri": "file:///path/to/resource",
          "name": "resource_name"
        },
        "id": "ResourceCallID"
      }
    },
    {
      "tool_call": {
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {
          "tool_name": "text_summarizer",
          "arguments": {
            "depends": "ResourceCallID",
            "text": "<ResourceCallID response>"
          }
        },
        "id": "ToolCallID"
      }
    }
  ]
}
```

Would turn into

```json
{
  "messages": [
    {
      "role": "system",
      "content": "compiled context document"
    },
    {
      "role": "assistant",
      "content": "resource request"
    },
    {
      "role": "system",
      "content": "<ResourceCallID response>"
    },
    {
      "role": "assistant",
      "content": " tool request using <ResourceCallID> response"
    },
    {
      "role": "tool",
      "content": "<ToolCallID response>"
    }
  ]
}
```

### Server Configuration

MCP Servers are configured in $HOME/.erasmus/mcp/config.json

```json
{
  "mcpServers": {
    "github": {
      "command": "./erasmus/mcp/github/server",
      "args": ["studio"],
      "env": {
        "GITHUB_PERSONAL_ACCESS_TOKEN": "${GITHUB_PERSONAL_ACCESS_TOKEN}"
      }
    }
  }
}
```
