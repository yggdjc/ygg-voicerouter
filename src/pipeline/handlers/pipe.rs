//! Pipe handler — write text to subprocess stdin, read stdout.

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

use crate::pipeline::handler::{Handler, HandlerResult};
use crate::pipeline::stage::StageContext;

pub struct PipeHandler;

impl Handler for PipeHandler {
    fn name(&self) -> &str { "pipe" }

    fn handle(&self, text: &str, ctx: &StageContext) -> Result<HandlerResult> {
        let cmd = ctx.get("command")
            .ok_or_else(|| anyhow::anyhow!("pipe handler requires 'command' param"))?;

        let effective_cmd = if cmd.contains("{text}") {
            cmd.replace("{text}", text)
        } else {
            cmd.to_string()
        };

        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(&effective_cmd)
            .stdin(if !cmd.contains("{text}") { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("failed to spawn pipe process")?;

        // Write text to stdin if no {text} template was used.
        if !cmd.contains("{text}") {
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(text.as_bytes()).ok();
            }
        }

        let output = child.wait_with_output().context("pipe process failed")?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            log::warn!("[pipe] stderr: {}", stderr.trim());
            bail!("pipe command failed: {effective_cmd}");
        }

        Ok(HandlerResult::Forward(stdout))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::stage::StageContext;
    use std::collections::HashMap;

    fn ctx(cmd: &str) -> StageContext {
        let mut params = HashMap::new();
        params.insert("command".into(), cmd.into());
        StageContext { stage_name: "test".into(), params }
    }

    #[test]
    fn pipe_captures_stdout() {
        let handler = PipeHandler;
        let ctx = ctx("cat");
        let result = handler.handle("hello", &ctx).unwrap();
        match result {
            HandlerResult::Forward(text) => assert_eq!(text.trim(), "hello"),
            _ => panic!("expected Forward"),
        }
    }

    #[test]
    fn pipe_with_template() {
        let handler = PipeHandler;
        let ctx = ctx("echo prefix-{text}");
        let result = handler.handle("world", &ctx).unwrap();
        match result {
            HandlerResult::Forward(text) => assert_eq!(text.trim(), "prefix-world"),
            _ => panic!("expected Forward"),
        }
    }
}
