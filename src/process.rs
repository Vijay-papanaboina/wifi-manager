//! Asynchronous subprocess boundary.
//!
//! Commands are launched through Gio so waiting for a process never blocks the
//! GTK main loop. Keeping this adapter small gives application code one place
//! to attach process diagnostics and future runner injection.

use std::ffi::{OsStr, OsString};

use gtk4::gio;

use crate::error::{AppError, AppResult};

/// Captured result of an asynchronously completed subprocess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandOutput {
    pub(crate) status: i32,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

impl CommandOutput {
    pub(crate) fn success(&self) -> bool {
        self.status == 0
    }

    pub(crate) fn into_success(self, program: &str) -> AppResult<Self> {
        if self.success() {
            Ok(self)
        } else {
            Err(AppError::command_failed(program, self.status, self.stderr))
        }
    }
}

fn new_subprocess<I, S>(
    program: &str,
    args: I,
    flags: gio::SubprocessFlags,
) -> AppResult<gio::Subprocess>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut argv: Vec<OsString> = Vec::new();
    argv.push(OsString::from(program));
    argv.extend(args.into_iter().map(|arg| arg.as_ref().to_os_string()));
    let argv_refs: Vec<&OsStr> = argv.iter().map(OsString::as_os_str).collect();

    gio::Subprocess::newv(&argv_refs, flags)
        .map_err(|error| AppError::process("spawn subprocess", error.to_string()))
}

/// Spawn a command without waiting for it. The returned handle may be dropped
/// after launch; the child process continues independently.
pub(crate) fn spawn<I, S>(program: &str, args: I) -> AppResult<gio::Subprocess>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    new_subprocess(program, args, gio::SubprocessFlags::NONE)
}

/// Run a command asynchronously, capturing UTF-8 stdout and stderr.
pub(crate) async fn run<I, S>(program: &str, args: I) -> AppResult<CommandOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let subprocess = new_subprocess(
        program,
        args,
        gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_PIPE,
    )?;

    let (stdout, stderr) = subprocess
        .communicate_future(None)
        .await
        .map_err(|error| AppError::process("communicate with subprocess", error.to_string()))?;

    Ok(CommandOutput {
        status: subprocess.exit_status(),
        stdout: stdout
            .map(|value| String::from_utf8_lossy(value.as_ref()).into_owned())
            .unwrap_or_default(),
        stderr: stderr
            .map(|value| String::from_utf8_lossy(value.as_ref()).into_owned())
            .unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_failure_diagnostics() {
        let output = CommandOutput {
            status: 7,
            stdout: String::new(),
            stderr: "permission denied".to_string(),
        };
        let error = output.into_success("example").expect_err("failure must be rejected");
        assert_eq!(error.to_string(), "example exited with status 7: permission denied");
    }
}
