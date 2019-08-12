use crate::depends::{
    failure::Error,
    futures::{executor::block_on, Future, Poll},
    log::{debug, info, warn},
};
use std::{borrow::Cow, ffi::OsStr, io::Read, path::PathBuf, process::Stdio};

pub struct Sh {}

impl Sh {
    const TAG: Cow<'static, str> = Cow::Borrowed("Sh");

    pub fn which<S: AsRef<OsStr>>(cmd: S) -> Option<PathBuf> {
        let which = std::process::Command::new("which")
            .arg(cmd.as_ref())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output();

        match which {
            Ok(output) => String::from_utf8(output.stdout).ok().map(|out| {
                // rm that trailing whitespace
                let trimmed = out.trim();
                debug!("[{}] which: {:?} -> {:?}", Self::TAG, cmd.as_ref(), trimmed);
                PathBuf::from(trimmed)
            }),
            Err(_) => {
                warn!(
                    "[{}] which: could not find command {:?} on path",
                    Self::TAG,
                    cmd.as_ref()
                );
                None
            }
        }
    }
}

pub struct Command {
    process: std::process::Child,
}

impl Command {
    const TAG: Cow<'static, str> = Cow::Borrowed("Command");

    pub fn new(cmd: &mut std::process::Command) -> Result<Self, Error> {
        let process = cmd.spawn()?;
        Ok(Command {
            process,
        })
    }

    pub fn wait(&mut self) -> Result<std::process::ExitStatus, Error> {
        Ok(self.process.wait()?)
    }

    pub fn wait_with_output(mut self) -> Result<std::process::Output, Error> {
        Ok(self.process.wait_with_output()?)
    }
}

impl Future for Command {
    type Output = std::process::Output;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context,
    ) -> Poll<Self::Output> {
        match self.process.try_wait() {
            Ok(Some(status)) => {
                info!("status READY");

                let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
                match ((&mut self.process).stdout.take(), (&mut self.process).stderr.take()) {
                    (None, None) => {}
                    (Some(mut out), None) => {
                        let res = out.read_to_end(&mut stdout);
                        res.unwrap();
                    }
                    (None, Some(mut err)) => {
                        let res = err.read_to_end(&mut stderr);
                        res.unwrap();
                    }
                    (Some(mut out), Some(mut err)) => {
                        let res = out
                            .read_to_end(&mut stdout)
                            .and_then(|size| err.read_to_end(&mut stderr).map(|s| size + s));
                        res.unwrap();
                    }
                }

                Poll::Ready(std::process::Output {
                    status,
                    stdout,
                    stderr,
                })
            }
            Ok(None) => {
                info!("status not ready yet, let's really wait");
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Err(e) => {
                info!("error attempting to wait: {}", e);
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }
}
