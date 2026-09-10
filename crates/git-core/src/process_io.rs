//! Stop-aware passive process pipes. Killing a process group cannot close pipe
//! descriptors retained by a helper which has started a separate session.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone)]
pub(super) struct PipeControl(Arc<PipeState>);

struct PipeState {
    stopped: AtomicBool,
    #[cfg(unix)]
    wake_reader: std::os::unix::net::UnixStream,
    #[cfg(unix)]
    wake_writer: std::os::unix::net::UnixStream,
}

impl PipeControl {
    fn new() -> Result<Self> {
        #[cfg(unix)]
        let (wake_reader, wake_writer) = std::os::unix::net::UnixStream::pair()?;
        Ok(Self(Arc::new(PipeState {
            stopped: AtomicBool::new(false),
            #[cfg(unix)]
            wake_reader,
            #[cfg(unix)]
            wake_writer,
        })))
    }

    pub(super) fn stop(&self) {
        self.0.stopped.store(true, Ordering::Release);
        #[cfg(unix)]
        let _ = self.0.wake_writer.shutdown(std::net::Shutdown::Write);
    }

    fn check(&self) -> std::io::Result<()> {
        if self.0.stopped.load(Ordering::Acquire) {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "Git pipe stopped",
            ))
        } else {
            Ok(())
        }
    }
}

pub(super) struct StopPipe<P> {
    pipe: P,
    control: PipeControl,
}

pub(super) trait PipeReady {
    fn wait_ready(&self, writing: bool, control: &PipeControl) -> std::io::Result<()>;
}

#[cfg(unix)]
impl<P: std::os::fd::AsFd> PipeReady for P {
    fn wait_ready(&self, writing: bool, control: &PipeControl) -> std::io::Result<()> {
        use rustix::event::{PollFd, PollFlags, poll};
        let interest = if writing {
            PollFlags::OUT
        } else {
            PollFlags::IN
        };
        // Socket EOF wakes every blocked reader/writer without consuming a
        // signal. Idle traversals need no periodic wakeups, and object requests
        // incur no fixed sleep while waiting for available data.
        let mut descriptors = [
            PollFd::new(self, interest),
            PollFd::new(&control.0.wake_reader, PollFlags::IN),
        ];
        match poll(&mut descriptors, None) {
            Ok(_) | Err(rustix::io::Errno::INTR) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

#[cfg(not(unix))]
impl<P> PipeReady for P {
    fn wait_ready(&self, _writing: bool, _control: &PipeControl) -> std::io::Result<()> {
        thread::sleep(Duration::from_millis(2));
        Ok(())
    }
}

impl<P: PipeReady> StopPipe<P> {
    fn retry<T>(
        &mut self,
        writing: bool,
        mut operation: impl FnMut(&mut P) -> std::io::Result<T>,
    ) -> std::io::Result<T> {
        loop {
            self.control.check()?;
            match operation(&mut self.pipe) {
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    self.pipe.wait_ready(writing, &self.control)?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                result => return result,
            }
        }
    }
}

impl<P: Read + PipeReady> Read for StopPipe<P> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.retry(false, |pipe| pipe.read(bytes))
    }
}

impl<P: Write + PipeReady> Write for StopPipe<P> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.retry(true, |pipe| pipe.write(bytes))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.retry(true, Write::flush)
    }
}

pub(super) struct ChildPipes {
    pub(super) input: Option<StopPipe<ChildStdin>>,
    pub(super) output: StopPipe<ChildStdout>,
    pub(super) error: Option<StopPipe<std::process::ChildStderr>>,
    pub(super) control: PipeControl,
}

impl ChildPipes {
    /// Prepare every descriptor before starting I/O threads. On setup failure,
    /// retain responsibility for terminating and reaping the spawned child.
    pub(super) fn take(child: &mut Child) -> Result<Self> {
        let result = (|| {
            let input = child.stdin.take();
            let output = child.stdout.take().context("Missing Git output pipe")?;
            let error = child.stderr.take();
            #[cfg(unix)]
            {
                use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
                fn nonblocking(pipe: &impl std::os::fd::AsFd) -> Result<()> {
                    fcntl_setfl(pipe, fcntl_getfl(pipe)? | OFlags::NONBLOCK)?;
                    Ok(())
                }
                nonblocking(&output)?;
                if let Some(pipe) = &input {
                    nonblocking(pipe)?;
                }
                if let Some(pipe) = &error {
                    nonblocking(pipe)?;
                }
            }
            let control = PipeControl::new()?;
            Ok(Self {
                input: input.map(|pipe| StopPipe {
                    pipe,
                    control: control.clone(),
                }),
                output: StopPipe {
                    pipe: output,
                    control: control.clone(),
                },
                error: error.map(|pipe| StopPipe {
                    pipe,
                    control: control.clone(),
                }),
                control,
            })
        })();
        if result.is_err() {
            terminate_process_group(child);
            let _ = child.kill();
            let _ = child.wait();
        }
        result
    }
}

#[cfg(all(test, unix))]
pub(super) mod fixtures {
    use super::*;

    pub(crate) struct DetachedPipeHolder {
        directory: tempfile::TempDir,
    }

    impl DetachedPipeHolder {
        pub(crate) fn command(input_held: bool) -> (Self, Command) {
            let fixture = Self {
                directory: tempfile::tempdir().unwrap(),
            };
            let mut command = Command::new("python3");
            command
                .args([
                    "-c",
                    "import os,sys,time
if os.fork() == 0:
 os.setsid()
 if sys.argv[2] == 'stdin':
  os.close(1)
  os.close(2)
 with open(sys.argv[1], 'w') as file: file.write(str(os.getpid()))
 time.sleep(5)
 os._exit(0)
while not os.path.exists(sys.argv[1]): time.sleep(0.001)
os._exit(0)
",
                ])
                .arg(fixture.marker())
                .arg(if input_held { "stdin" } else { "output" });
            (fixture, command)
        }

        fn marker(&self) -> PathBuf {
            self.directory.path().join("detached-pid")
        }

        pub(crate) fn wait_ready(&self) {
            let started = Instant::now();
            while !std::fs::read_to_string(self.marker()).is_ok_and(|pid| !pid.is_empty()) {
                assert!(started.elapsed() < Duration::from_secs(3));
                thread::sleep(Duration::from_millis(2));
            }
        }
    }

    impl Drop for DetachedPipeHolder {
        fn drop(&mut self) {
            use rustix::process::{Pid, Signal, kill_process};
            if let Ok(pid) = std::fs::read_to_string(self.marker())
                && let Ok(pid) = pid.parse::<i32>()
                && let Some(pid) = Pid::from_raw(pid)
            {
                let _ = kill_process(pid, Signal::KILL);
            }
        }
    }
}
