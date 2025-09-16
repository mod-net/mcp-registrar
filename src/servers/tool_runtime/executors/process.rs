use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command as TokioCommand;

use crate::error::Error;
use crate::servers::tool_runtime::{Executor, Policy, ToolRuntime};
use tracing::{debug, info, warn};

#[derive(Debug)]
pub struct ProcessExecutor;

#[async_trait::async_trait]
impl Executor for ProcessExecutor {
    async fn invoke(
        &self,
        tool_id: &str,
        runtime: &ToolRuntime,
        args_json: &serde_json::Value,
        policy: &Policy,
    ) -> Result<serde_json::Value, Error> {
        let cfg = match runtime {
            ToolRuntime::Process(cfg) => cfg,
            _ => return Err(Error::InvalidState("ProcessExecutor received non-process runtime".into())),
        };

        debug!("spawning process tool {} -> {:?} {:?}", tool_id, cfg.command, cfg.args);
        let mut cmd = TokioCommand::new(&cfg.command);
        if !cfg.args.is_empty() {
            cmd.args(&cfg.args);
        }
        // TODO: env_allowlist enforcement; network/filesystem sandbox to be added later.
        let mut child = cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(Error::from)?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::InvalidState("stdin missing".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::InvalidState("stdout missing".into()))?;

        let mut reader = BufReader::new(stdout).lines();
        let request = serde_json::json!({ "arguments": args_json });
        let mut line = serde_json::to_string(&request)?;
        line.push('\n');
        stdin.write_all(line.as_bytes()).await.map_err(Error::from)?;
        drop(stdin);

        let started = std::time::Instant::now();
        let next_res = tokio::time::timeout(
            std::time::Duration::from_millis(policy.timeout_ms),
            reader.next_line(),
        )
        .await
        .map_err(|_| {
            warn!("process tool {} timed out after {} ms", tool_id, policy.timeout_ms);
            Error::InvalidState(format!("tool {} timed out", tool_id))
        })?;

        let opt_line = next_res.map_err(|e| Error::Other(Box::new(e)))?;
        let line = opt_line.ok_or_else(|| Error::InvalidState("empty tool response".into()))?;
        if line.len() > policy.max_output_bytes {
            return Err(Error::InvalidState("tool output too large".into()));
        }
        let duration_ms = started.elapsed().as_millis();
        let bytes = line.len();
        info!("process tool {} completed in {} ms ({} bytes)", tool_id, duration_ms, bytes);
        let resp: serde_json::Value = serde_json::from_str(&line)?;
        crate::monitoring::TOOL_METRICS.record(duration_ms as u64, bytes as u64, false);
        Ok(resp)
    }
}
