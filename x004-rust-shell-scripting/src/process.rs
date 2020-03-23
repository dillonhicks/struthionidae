use crate::deps::{
    failure::Error,
    futures::{executor::block_on, Future, task::{Poll, SpawnExt}, io::AsyncReadExt, io::AsyncRead},
    struthionidae::logging::{info, error, debug, trace},
};
use std::process as imp;
use std::{borrow::Cow, fmt, ffi::OsStr, fs, io::Read, path::{Path, PathBuf}};
use std::os::unix::io::{IntoRawFd, FromRawFd};
use std::os::unix::io::RawFd;
use futures::stream::{FuturesOrdered, FuturesUnordered};

use crate::pipe::{PipeReader, PipeWriter, Pipe};
use std::pin::Pin;
use futures::StreamExt;
use std::iter::FromIterator;


#[derive(Debug, Default)]
struct Buffers {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,

}

#[derive(Debug)]
pub struct Process {
    process: std::process::Child,
    stdout: Option<PipeReader>,
    stderr: Option<PipeReader>,
    io_bufs: Buffers,
}


impl Process {
    const TAG: Cow<'static, str> = Cow::Borrowed("Command");

    pub fn new(cmd: &mut Command) -> Result<Self, Error> {
        let child = cmd.inner.spawn()?;
        let mut proc = Process {
            process: child,
            stdout: None,
            stderr: None,
            io_bufs: Default::default(),
        };

        // std::mem::swap(&mut proc.stdin, &mut cmd.stdin);
        std::mem::swap(&mut proc.stdout, &mut cmd.stdout);
        std::mem::swap(&mut proc.stderr, &mut cmd.stderr);
        Ok(proc)
    }

    pub fn wait(&mut self) -> Result<std::process::ExitStatus, Error> {
        Ok(self.process.wait()?)
    }

    pub fn wait_with_output(mut self) -> Result<std::process::Output, Error> {
        Ok(self.process.wait_with_output()?)
    }
}


impl Future for Process {
    type Output = std::process::Output;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context,
    ) -> Poll<Self::Output> {
        match self.stdout.as_mut() {
            Some(out) => {
                let mut buf = [0u8; 4096];

                let outpin: Pin<&mut PipeReader> = unsafe {
                    Pin::new_unchecked(out)
                };
                match outpin.poll_read(cx, &mut buf) {
                    Poll::Ready(Ok(s)) => {
                        debug!("stdout read {} bytes", s);
                        info!("stdout read: {}", String::from_utf8_lossy(&buf[0..s]));
                        self.io_bufs.stdout.extend_from_slice(&buf[..s]);
                    }
                    Poll::Ready(Err(e)) => {
                        error!("stdout read {:?}", e);
                    }
                    _ => {}
                }
            }
            None => {}
        };

        match self.stderr.as_mut() {
            Some(out) => {
                let mut buf = [0u8; 4096];

                let outpin: Pin<&mut PipeReader> = unsafe {
                    Pin::new_unchecked(out)
                };
                match outpin.poll_read(cx, &mut buf) {
                    Poll::Ready(Ok(s)) => {
                        debug!("stderr read {} bytes", s);
                        info!("stderr read: {}", String::from_utf8_lossy(&buf[0..s]));
                        self.io_bufs.stderr.extend_from_slice(&buf[..s]);
                    }
                    Poll::Ready(Err(e)) => {
                        error!("stderr read {:?}", e);
                    }
                    _ => {}
                }
            }
            None => {}
        };

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

                let mut io_bufs = Buffers::default();

                std::mem::swap(&mut io_bufs, &mut self.io_bufs);

                Poll::Ready(std::process::Output {
                    status,
                    stdout: io_bufs.stdout,
                    stderr: io_bufs.stderr,
                })
            }
            Ok(None) => {
                trace!("status not ready yet, let's really wait");
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Err(e) => {
                error!("error attempting to wait: {}", e);
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }
}


#[derive(Debug, Copy, Clone, PartialEq)]
enum IoKind {
    Piped,
    Inherit,
    Null,
    FileDescriptor(RawFd),
    // Sink(Fd),
    // Source(Fd)
}


#[derive(Debug, Copy, Clone, PartialEq)]
enum Fd {
    DevNull,
    Stdin,
    Stdout,
    Stderr,
    Other(RawFd),
}


impl From<RawFd> for Fd {
    fn from(fd: RawFd) -> Self {
        match fd {
            std::i32::MIN..=-1 => Fd::DevNull,
            0 => Fd::Stdin,
            1 => Fd::Stdout,
            2 => Fd::Stderr,
            _ => Fd::Other(fd)
        }
    }
}

#[derive(Copy, Clone, PartialEq)]
pub struct Stdio {
    io_kind: IoKind
}

impl fmt::Debug for Stdio {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Stdio({:?})", &self.io_kind)
    }
}

impl Into<imp::Stdio> for Stdio {
    fn into(self) -> imp::Stdio {
        match self.io_kind {
            IoKind::Null => imp::Stdio::null(),
            IoKind::Inherit => imp::Stdio::inherit(),
            IoKind::FileDescriptor(file) => unsafe { imp::Stdio::from(fs::File::from_raw_fd(file)) },
            IoKind::Piped => imp::Stdio::piped(),
//            IoKind::Sink(fd) => {imp::Stdio::from()}
//            IoKind::Source(fd) => {}
        }
    }
}

impl Stdio {
    pub fn null() -> Self {
        Stdio {
            io_kind: IoKind::Null
        }
    }
    pub fn inherit() -> Self {
        Stdio {
            io_kind: IoKind::Inherit
        }
    }
    pub fn piped() -> Self {
        Stdio {
            io_kind: IoKind::Piped
        }
    }

    pub fn redirect_to_out() -> Self {
        Stdio {
            io_kind: IoKind::Piped
        }
    }

    pub fn from_file(file: fs::File) -> Self {
        Stdio {
            io_kind: IoKind::FileDescriptor(file.into_raw_fd())
        }
    }
}


/// A process builder, providing fine-grained control
/// over how a new process should be spawned.
///
/// A default configuration can be
/// generated using `Command::new(program)`, where `program` gives a path to the
/// program to be executed. Additional builder methods allow the configuration
/// to be changed (for example, by adding arguments) prior to spawning:
///
/// ```no_run
/// use std::process::Command;
///
/// let output = if cfg!(target_os = "windows") {
///     Command::new("cmd")
///             .args(&["/C", "echo hello"])
///             .output()
///             .expect("failed to execute process")
/// } else {
///     Command::new("sh")
///             .arg("-c")
///             .arg("echo hello")
///             .output()
///             .expect("failed to execute process")
/// };
///
/// let hello = output.stdout;
/// ```
///
/// `Command` can be reused to spawn multiple processes. The builder methods
/// change the command without needing to immediately spawn the process.
///
/// ```no_run
/// use std::process::Command;
///
/// let mut echo_hello = Command::new("sh");
/// echo_hello.arg("-c")
///           .arg("echo hello");
/// let hello_1 = echo_hello.output().expect("failed to execute process");
/// let hello_2 = echo_hello.output().expect("failed to execute process");
/// ```
///
/// Similarly, you can call builder methods after spawning a process and then
/// spawn a new process with the modified settings.
///
/// ```no_run
/// use std::process::Command;
///
/// let mut list_dir = Command::new("ls");
///
/// // Execute `ls` in the current directory of the program.
/// list_dir.status().expect("process failed to execute");
///
/// println!();
///
/// // Change `ls` to execute in the root directory.
/// list_dir.current_dir("/");
///
/// // And then execute `ls` again but in the root directory.
/// list_dir.status().expect("process failed to execute");
/// ```
#[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
pub struct Command {
    inner: imp::Command,
    stdin: Option<PipeWriter>,
    stdout: Option<PipeReader>,
    stderr: Option<PipeReader>,
}


impl Command {
    /// Constructs a new `Command` for launching the program at
    /// path `program`, with the following default configuration:
    ///
    /// * No arguments to the program
    /// * Inherit the current process's environment
    /// * Inherit the current process's working directory
    /// * Inherit stdin/stdout/stderr for `spawn` or `status`, but create pipes for `output`
    ///
    /// Builder methods are provided to change these defaults and
    /// otherwise configure the process.
    ///
    /// If `program` is not an absolute path, the `PATH` will be searched in
    /// an OS-defined way.
    ///
    /// The search path to be used may be controlled by setting the
    /// `PATH` environment variable on the Command,
    /// but this has some implementation limitations on Windows
    /// (see issue #37519).
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```no_run
    /// use std::process::Command;
    ///
    /// Command::new("sh")
    ///         .spawn()
    ///         .expect("sh command failed to start");
    /// ```
    #[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
    pub fn new<S: AsRef<OsStr>>(program: S) -> Command {
        Command { inner: imp::Command::new(program.as_ref()), stdin: None, stdout: None, stderr: None }
    }

    /// Adds an argument to pass to the program.
    ///
    /// Only one argument can be passed per use. So instead of:
    ///
    /// ```no_run
    /// # std::process::Command::new("sh")
    /// .arg("-C /path/to/repo")
    /// # ;
    /// ```
    ///
    /// usage would be:
    ///
    /// ```no_run
    /// # std::process::Command::new("sh")
    /// .arg("-C")
    /// .arg("/path/to/repo")
    /// # ;
    /// ```
    ///
    /// To pass multiple arguments see [`args`].
    ///
    /// [`args`]: #method.args
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```no_run
    /// use std::process::Command;
    ///
    /// Command::new("ls")
    ///         .arg("-l")
    ///         .arg("-a")
    ///         .spawn()
    ///         .expect("ls command failed to start");
    /// ```
    #[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
    pub fn arg<S: AsRef<OsStr>>(mut self, arg: S) -> Command {
        self.inner.arg(arg.as_ref());
        self
    }

    /// Adds multiple arguments to pass to the program.
    ///
    /// To pass a single argument see [`arg`].
    ///
    /// [`arg`]: #method.arg
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```no_run
    /// use std::process::Command;
    ///
    /// Command::new("ls")
    ///         .args(&["-l", "-a"])
    ///         .spawn()
    ///         .expect("ls command failed to start");
    /// ```
    #[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
    pub fn args<I, S>(mut self, args: I) -> Command
        where I: IntoIterator<Item=S>, S: AsRef<OsStr>
    {
        for arg in args {
            self = self.arg(arg.as_ref());
        }
        self
    }

    /// Inserts or updates an environment variable mapping.
    ///
    /// Note that environment variable names are case-insensitive (but case-preserving) on Windows,
    /// and case-sensitive on all other platforms.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```no_run
    /// use std::process::Command;
    ///
    /// Command::new("ls")
    ///         .env("PATH", "/bin")
    ///         .spawn()
    ///         .expect("ls command failed to start");
    /// ```
    #[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
    pub fn env<K, V>(mut self, key: K, val: V) -> Command
        where K: AsRef<OsStr>, V: AsRef<OsStr>
    {
        self.inner.env(key.as_ref(), val.as_ref());
        self
    }

    /// Adds or updates multiple environment variable mappings.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```no_run
    /// use std::process::{Command, Stdio};
    /// use std::env;
    /// use std::collections::HashMap;
    ///
    /// let filtered_env : HashMap<String, String> =
    ///     env::vars().filter(|&(ref k, _)|
    ///         k == "TERM" || k == "TZ" || k == "LANG" || k == "PATH"
    ///     ).collect();
    ///
    /// Command::new("printenv")
    ///         .stdin(Stdio::null())
    ///         .stdout(Stdio::inherit())
    ///         .env_clear()
    ///         .envs(&filtered_env)
    ///         .spawn()
    ///         .expect("printenv failed to start");
    /// ```
    #[cfg_attr(feature = "ruststd", stable(feature = "command_envs", since = "1.19.0"))]
    pub fn envs<I, K, V>(mut self, vars: I) -> Command
        where I: IntoIterator<Item=(K, V)>, K: AsRef<OsStr>, V: AsRef<OsStr>
    {
        for (ref key, ref val) in vars {
            self.inner.env(key.as_ref(), val.as_ref());
        }
        self
    }

    /// Removes an environment variable mapping.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```no_run
    /// use std::process::Command;
    ///
    /// Command::new("ls")
    ///         .env_remove("PATH")
    ///         .spawn()
    ///         .expect("ls command failed to start");
    /// ```
    #[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
    pub fn env_remove<K: AsRef<OsStr>>(mut self, key: K) -> Command {
        self.inner.env_remove(key.as_ref());
        self
    }

    /// Clears the entire environment map for the child process.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```no_run
    /// use std::process::Command;
    ///
    /// Command::new("ls")
    ///         .env_clear()
    ///         .spawn()
    ///         .expect("ls command failed to start");
    /// ```
    #[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
    pub fn env_clear(mut self) -> Command {
        self.inner.env_clear();
        self
    }

    /// Sets the working directory for the child process.
    ///
    /// # Platform-specific behavior
    ///
    /// If the program path is relative (e.g., `"./script.sh"`), it's ambiguous
    /// whether it should be interpreted relative to the parent's working
    /// directory or relative to `current_dir`. The behavior in this case is
    /// platform specific and unstable, and it's recommended to use
    /// [`canonicalize`] to get an absolute program path instead.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```no_run
    /// use std::process::Command;
    ///
    /// Command::new("ls")
    ///         .current_dir("/bin")
    ///         .spawn()
    ///         .expect("ls command failed to start");
    /// ```
    ///
    /// [`canonicalize`]: ../fs/fn.canonicalize.html
    #[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
    pub fn current_dir<P: AsRef<Path>>(mut self, dir: P) -> Command {
        self.inner.current_dir(dir);
        self
    }

    /// Configuration for the child process's standard input (stdin) handle.
    ///
    /// Defaults to [`inherit`] when used with `spawn` or `status`, and
    /// defaults to [`piped`] when used with `output`.
    ///
    /// [`inherit`]: struct.Stdio.html#method.inherit
    /// [`piped`]: struct.Stdio.html#method.piped
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```no_run
    /// use std::process::{Command, Stdio};
    ///
    /// Command::new("ls")
    ///         .stdin(Stdio::null())
    ///         .spawn()
    ///         .expect("ls command failed to start");
    /// ```
    #[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
    pub fn stdin(mut self, cfg: Stdio) -> Command {
        match cfg.io_kind {
            IoKind::Piped => {
                let (r, w) = Pipe::new().unwrap();
                self.stdin = Some(w);
                self.inner.stdin(imp::Stdio::from(unsafe { fs::File::from_raw_fd(r.into_raw_fd()) }));
                self
            }
            IoKind::FileDescriptor(fd) => {
                self.inner.stdin(imp::Stdio::from(unsafe { fs::File::from_raw_fd(fd) }));
                self
            }
            _ => {
                self.inner.stdout(Into::<imp::Stdio>::into(cfg));
                self
            }
        }
    }

    /// Configuration for the child process's standard output (stdout) handle.
    ///
    /// Defaults to [`inherit`] when used with `spawn` or `status`, and
    /// defaults to [`piped`] when used with `output`.
    ///
    /// [`inherit`]: struct.Stdio.html#method.inherit
    /// [`piped`]: struct.Stdio.html#method.piped
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```no_run
    /// use std::process::{Command, Stdio};
    ///
    /// Command::new("ls")
    ///         .stdout(Stdio::null())
    ///         .spawn()
    ///         .expect("ls command failed to start");
    /// ```
    #[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
    pub fn stdout(mut self, cfg: Stdio) -> Command {
        match cfg.io_kind {
            IoKind::Piped => {
                let (r, w) = Pipe::new().unwrap();
                self.stdout = Some(r);
                self.inner.stdout(imp::Stdio::from(unsafe { fs::File::from_raw_fd(w.into_raw_fd()) }));
                self
            }
            _ => {
                self.inner.stdout(Into::<imp::Stdio>::into(cfg));
                self
            }
        }
    }

    /// Configuration for the child process's standard error (stderr) handle.
    ///
    /// Defaults to [`inherit`] when used with `spawn` or `status`, and
    /// defaults to [`piped`] when used with `output`.
    ///
    /// [`inherit`]: struct.Stdio.html#method.inherit
    /// [`piped`]: struct.Stdio.html#method.piped
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```no_run
    /// use std::process::{Command, Stdio};
    ///
    /// Command::new("ls")
    ///         .stderr(Stdio::null())
    ///         .spawn()
    ///         .expect("ls command failed to start");
    /// ```
    #[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
    pub fn stderr(mut self, cfg: Stdio) -> Command {
        match cfg.io_kind {
            IoKind::Piped => {
                let (r, w) = Pipe::new().unwrap();
                self.stderr = Some(r);
                self.inner.stderr(imp::Stdio::from(unsafe { fs::File::from_raw_fd(w.into_raw_fd()) }));
                self
            }
            _ => {
                self.inner.stderr(Into::<imp::Stdio>::into(cfg));
                self
            }
        }
    }

    /// Executes the command as a child process, returning a handle to it.
    ///
    /// By default, stdin, stdout and stderr are inherited from the parent.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```no_run
    /// use std::process::Command;
    ///
    /// Command::new("ls")
    ///         .spawn()
    ///         .expect("ls command failed to start");
    /// ```
    #[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
    pub fn spawn(&mut self) -> Result<Process, Error> {
        Process::new(self)
    }

    /// Executes the command as a child process, waiting for it to finish and
    /// collecting all of its output.
    ///
    /// By default, stdout and stderr are captured (and used to provide the
    /// resulting output). Stdin is not inherited from the parent and any
    /// attempt by the child process to read from the stdin stream will result
    /// in the stream immediately closing.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::process::Command;
    /// use std::io::{self, Write};
    /// let output = Command::new("/bin/cat")
    ///                      .arg("file.txt")
    ///                      .output()
    ///                      .expect("failed to execute process");
    ///
    /// println!("status: {}", output.status);
    /// io::stdout().write_all(&output.stdout).unwrap();
    /// io::stderr().write_all(&output.stderr).unwrap();
    ///
    /// assert!(output.status.success());
    /// ```
    #[cfg(feature = "DISABLED")]
    #[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
    pub fn output(&mut self) -> io::Result<Output> {
        self.inner.spawn(imp::Stdio::MakePipe, false).map(Child::from_inner)
            .and_then(|p| p.wait_with_output())
    }

    /// Executes a command as a child process, waiting for it to finish and
    /// collecting its exit status.
    ///
    /// By default, stdin, stdout and stderr are inherited from the parent.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::process::Command;
    ///
    ///                      .expect("failed to execute process");
    ///
    /// let status = Command::new("/bin/cat")
    ///                      .arg("file.txt")
    ///                      .status()
    /// println!("process exited with: {}", status);
    ///
    /// assert!(status.success());
    /// ```
    #[cfg(feature = "DISABLED")]
    #[cfg_attr(feature = "ruststd", stable(feature = "process", since = "1.0.0"))]
    pub fn status(&mut self) -> io::Result<ExitStatus> {
        self.inner.spawn(imp::Stdio::Inherit, true).map(Child::from_inner)
            .and_then(|mut p| p.wait())
    }
}

#[cfg_attr(feature = "ruststd", stable(feature = "rust1", since = "1.0.0"))]
impl fmt::Debug for Command {
    /// Format the program and arguments of a Command for display. Any
    /// non-utf8 data is lossily converted using the utf8 replacement
    /// character.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.fmt(f)
    }
}

#[derive(Debug)]
pub struct JobOutput { pub outputs: Vec<std::process::Output> }

impl Default for JobOutput {
    fn default() -> Self {
        JobOutput {
            outputs: vec![]
        }
    }
}


#[derive(Debug)]
pub struct Job {
    jobs: ::futures::stream::Collect<FuturesOrdered<Process>, Vec<std::process::Output>>,
}


impl Future for Job {
    type Output = JobOutput;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context,
    ) -> Poll<Self::Output> {
        use futures::ready;
        let jobs = &mut self.jobs;

        let pinned: Pin<&mut ::futures::stream::Collect<FuturesOrdered<Process>, Vec<std::process::Output>>> = unsafe {
            Pin::new_unchecked(jobs)
        };
        Poll::Ready(JobOutput { outputs: ready!(pinned.poll(cx)) })
    }
}

#[derive(Debug)]
pub struct Pipeline {
    commands: Vec<Command>,
}

impl Pipeline {
    pub fn new(cmd: Command) -> Self {
        let cmd = cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        Self {
            commands: vec![cmd]
        }
    }

    pub fn add(mut self, cmd: Command) -> Self {
        let mut stdout = None;
        std::mem::swap(&mut stdout, &mut self.commands.last_mut().unwrap().stdout);

        let stdin = match stdout {
            Some(pipe) => {
                Stdio {
                    io_kind: IoKind::FileDescriptor(pipe.into_raw_fd())
                }
            }
            None => Stdio::null()
        };

        let cmd = cmd.stdin(stdin)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        self.commands.push(cmd);
        self
    }

    pub fn spawn(mut self) -> Result<Job, Error> {
        let mut jobs = std::collections::VecDeque::new();

        for mut cmd in self.commands.drain(..) {
            jobs.push_back(cmd.spawn()?);
        }

        Ok(Job {
            jobs: FuturesOrdered::from_iter(jobs.into_iter()).collect()
        })
    }
}


#[cfg(test)]
async fn run_async_command_test() -> Vec<std::process::Output> {
    use ::futures::join;
    use ::futures::stream::StreamExt;

    let mut tasks = FuturesUnordered::new();

    let mut cmd =
        Command::new("../x004-rust-shell-scripting/print-alphabet.sh")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
    tasks.push(cmd);


    let mut cmd =
        Command::new("/bin/ls")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .arg("no-exist")
            .spawn()
            .unwrap();
    tasks.push(cmd);

    let mut cmd =
        Command::new("/bin/ls")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .arg("--help")
            .spawn()
            .unwrap();
    tasks.push(cmd);

    let mut cmd =
        Command::new("/bin/ls")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .arg("-1")
            .spawn()
            .unwrap();
    tasks.push(cmd);

    tasks.collect::<Vec<_>>().await
}

// ls -1 | sort




#[cfg(test)]
async fn run_pipeline_test() -> JobOutput {
    let job = Pipeline::new(
        Command::new("/usr/bin/seq")
            .arg("1")
            .arg("500")
        )
        .add(
            Command::new("/usr/bin/tr")
                .arg("-d")
                .arg(r#"\n"#)
        )
        .add(
            Command::new("/usr/bin/rev")
        )
        .spawn()
        .unwrap();

    job.await
}


#[test]
fn test_run_pipeline_test() {
    crate::deps::struthionidae::logging::initialize();
    use std::sync::mpsc;

    let (tx, rx) = mpsc::sync_channel(2);

    let mut pool = futures::executor::ThreadPoolBuilder::new()
        .name_prefix("pipeline")
        .pool_size(1)
        .create()
        .unwrap();
    pool.spawn(async move {
        let task_output = run_pipeline_test().await;
        tx.send(task_output).unwrap();
    }).unwrap();

    info!("{:#?}", rx.into_iter().next().unwrap());
}

#[test]
fn test_run_async_command_test() {
    crate::deps::struthionidae::logging::initialize();
    use std::sync::mpsc;

    let (tx, rx) = mpsc::sync_channel(2);

    let mut pool = futures::executor::ThreadPoolBuilder::new()
        .name_prefix("cmd_test")
        .pool_size(1)
        .create()
        .unwrap();
    pool.spawn(async move {
        let task_output = run_async_command_test().await;
        tx.send(task_output).unwrap();
    }).unwrap();

    info!("{:#?}", rx.into_iter().next().unwrap());
}