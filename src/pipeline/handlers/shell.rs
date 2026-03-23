//! Shell handler — execute text as a shell command or apply command template.

use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::pipeline::handler::{Handler, HandlerResult};
use crate::pipeline::stage::StageContext;

pub struct ShellHandler;

impl Handler for ShellHandler {
    fn name(&self) -> &str {
        "shell"
    }

    fn handle(&self, text: &str, ctx: &StageContext) -> Result<HandlerResult> {
        let cmd = match ctx.get("command") {
            Some(tpl) => {
                let encoded = url_encode(text);
                tpl.replace("{text}", &encoded)
            }
            None => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    bail!("shell handler received empty command");
                }
                trimmed.to_string()
            }
        };

        log::info!("[shell] executing: {:?}", cmd);

        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(&cmd)
            .output()
            .context("failed to spawn shell process")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !stdout.is_empty() {
            log::info!("[shell] stdout: {}", stdout.trim_end());
        }
        if !stderr.is_empty() {
            log::warn!("[shell] stderr: {}", stderr.trim_end());
        }

        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            bail!("shell command exited with status {code}: {cmd:?}");
        }

        Ok(HandlerResult::Done)
    }
}

fn url_encode(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(b as char);
            }
            b' ' => result.push('+'),
            _ => {
                result.push('%');
                result.push_str(&format!("{b:02X}"));
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::handler::HandlerResult;
    use crate::pipeline::stage::StageContext;
    use std::collections::HashMap;

    fn ctx_with_command(cmd: &str) -> StageContext {
        let mut params = HashMap::new();
        params.insert("command".into(), cmd.into());
        StageContext { stage_name: "test".into(), params }
    }

    #[test]
    fn shell_handler_name() {
        let handler = ShellHandler;
        assert_eq!(handler.name(), "shell");
    }

    #[test]
    fn shell_executes_command_template() {
        let handler = ShellHandler;
        let ctx = ctx_with_command("echo {text}");
        let result = handler.handle("hello", &ctx).unwrap();
        assert!(matches!(result, HandlerResult::Done));
    }

    #[test]
    fn shell_executes_raw_text_without_template() {
        let handler = ShellHandler;
        let ctx = StageContext { stage_name: "test".into(), params: HashMap::new() };
        let result = handler.handle("echo raw", &ctx).unwrap();
        assert!(matches!(result, HandlerResult::Done));
    }

    #[test]
    fn shell_url_encodes_text_in_template() {
        let handler = ShellHandler;
        let ctx = ctx_with_command("echo '{text}'");
        let result = handler.handle("hello world", &ctx).unwrap();
        assert!(matches!(result, HandlerResult::Done));
    }

    #[test]
    fn shell_empty_text_with_no_template_errors() {
        let handler = ShellHandler;
        let ctx = StageContext { stage_name: "test".into(), params: HashMap::new() };
        let result = handler.handle("", &ctx);
        assert!(result.is_err());
    }
}
