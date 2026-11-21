//! Stop-aware passive process pipes. Killing a process group cannot close pipe
//! descriptors retained by a helper which has started a separate session.
//!
//! Also what a bounded wait for one child is built from: the guard by which
//! an owned thread says that it ended, the helper that sees the child's exit
//! without reaping it, and the one timed wait that either of them ends.
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

/// One of the threads a wait owns, as that wait's events name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WaitPart {
    Input,
    Output,
    Error,
    Exit,
}

impl WaitPart {
    fn thread_name(self) -> &'static str {
        match self {
            Self::Input => "git-stdin",
            Self::Output => "git-stdout",
            Self::Error => "git-stderr",
            Self::Exit => "git-exit",
        }
    }

    fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// How a wait learns that its child has exited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ExitWatch {
    /// A helper thread sees the exit without reaping it and reports it.
    Readiness,
    /// The wait asks `try_wait` at every tick, once both streams have ended.
    Poll,
    /// A report that arrives while the child still runs, as a helper's could
    /// where the platform answers for a stopped child.
    #[cfg(test)]
    ReportsEarly,
}

/// What a test makes of one owned thread, as an exhausted host or a defect
/// in the thread's own work would.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Fault {
    CannotStart(WaitPart),
    Panics(WaitPart),
}

/// What one wait runs with. The waits pass their own tick and nothing else;
/// a test passes a tick, a way of seeing the exit and a fault of its own.
pub(super) struct WaitSeam {
    /// How often a wait that has to look wakes to look: for a cancellation
    /// while a token is installed, for the exit while nothing reports it.
    pub(super) tick: Duration,
    pub(super) exit: ExitWatch,
    /// Held by every thread the wait owns and by nothing else of the wait's,
    /// so that whoever keeps a clone counts 1 once they have all ended.
    pub(super) owned: Arc<()>,
    #[cfg(test)]
    pub(super) fault: Option<Fault>,
}

impl WaitSeam {
    pub(super) fn new(tick: Duration) -> Self {
        Self {
            tick,
            exit: if cfg!(unix) {
                ExitWatch::Readiness
            } else {
                ExitWatch::Poll
            },
            owned: Arc::new(()),
            #[cfg(test)]
            fault: None,
        }
    }
}

/// What a wait does next, as its own order judged what it saw after a wake.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WaitStep {
    Cancelled,
    WaitFailed,
    Complete,
    TimedOut,
    Wait,
}

/// Starts the threads one wait owns. The exit watcher, started last, takes it,
/// so that once the wait begins only those threads keep its events open.
pub(super) struct WaitSignals {
    events: mpsc::Sender<WaitPart>,
    owned: Arc<()>,
    #[cfg(test)]
    fault: Option<Fault>,
}

/// Says that the thread holding it has ended, however it ended: a return and
/// a panic drop it alike.
struct Ended {
    part: WaitPart,
    events: mpsc::Sender<WaitPart>,
    _owned: Arc<()>,
}

impl Drop for Ended {
    fn drop(&mut self) {
        let _ = self.events.send(self.part);
    }
}

/// The events of one wait: which of its owned threads have ended.
pub(super) struct WaitEvents {
    events: mpsc::Receiver<WaitPart>,
    ended: u8,
    closed: bool,
}

pub(super) fn wait_events(seam: &WaitSeam) -> (WaitSignals, WaitEvents) {
    let (sender, receiver) = mpsc::channel();
    (
        WaitSignals {
            events: sender,
            owned: Arc::clone(&seam.owned),
            #[cfg(test)]
            fault: seam.fault,
        },
        WaitEvents {
            events: receiver,
            ended: 0,
            closed: false,
        },
    )
}

impl WaitSignals {
    fn ended(&self, part: WaitPart) -> Ended {
        Ended {
            part,
            events: self.events.clone(),
            _owned: Arc::clone(&self.owned),
        }
    }

    /// Starts `run` as the wait's `part`. The thread keeps its result in its
    /// handle and reports only that it ended. One that cannot start is an
    /// error for the wait to clean up after, never a panic beside a live
    /// child; it counts as ended.
    pub(super) fn start<T: Send + 'static>(
        &self,
        part: WaitPart,
        run: impl FnOnce() -> T + Send + 'static,
    ) -> std::io::Result<JoinHandle<T>> {
        let ended = self.ended(part);
        #[cfg(test)]
        let panics = self.fault == Some(Fault::Panics(part));
        #[cfg(test)]
        if self.fault == Some(Fault::CannotStart(part)) {
            return Err(std::io::Error::other("the fixture starts no such thread"));
        }
        thread::Builder::new()
            .name(part.thread_name().into())
            .spawn(move || {
                let _ended = ended;
                #[cfg(test)]
                assert!(!panics, "the fixture panics this thread");
                run()
            })
    }
}

impl WaitEvents {
    pub(super) fn ended(&self, part: WaitPart) -> bool {
        self.ended & part.bit() != 0
    }

    /// Blocks until an owned thread ends or `timeout` passes, whichever is
    /// first, and takes in every event that has arrived by then. Once every
    /// owned thread has ended no event can come: the wait then only lets
    /// `timeout` pass, which the caller keeps to a tick.
    pub(super) fn next(&mut self, timeout: Duration) {
        if self.closed {
            thread::sleep(timeout);
            return;
        }
        match self.events.recv_timeout(timeout) {
            Ok(part) => self.ended |= part.bit(),
            Err(mpsc::RecvTimeoutError::Timeout) => return,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.closed = true;
                return;
            }
        }
        loop {
            match self.events.try_recv() {
                Ok(part) => self.ended |= part.bit(),
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.closed = true;
                    return;
                }
            }
        }
    }
}

/// Learns of a child's exit for one wait. Its report is a hint and never a
/// status: the status is only ever taken with `Child::try_wait`, which cannot
/// block, and a report that turns out false leaves the wait polling.
pub(super) struct ExitWatcher {
    helper: Option<JoinHandle<()>>,
    polling: bool,
}

impl ExitWatcher {
    /// Starts watching `child` as the seam says, as the last of the wait's
    /// owned threads. Where no helper can run (it could not start, or the
    /// platform has none) the wait polls.
    pub(super) fn start(child: &Child, signals: WaitSignals, seam: &WaitSeam) -> Self {
        match seam.exit {
            ExitWatch::Readiness => {
                let helper = watch_exit(child.id(), &signals);
                Self {
                    polling: helper.is_none(),
                    helper,
                }
            }
            ExitWatch::Poll => Self {
                helper: None,
                polling: true,
            },
            #[cfg(test)]
            ExitWatch::ReportsEarly => {
                drop(signals.ended(WaitPart::Exit));
                Self {
                    helper: None,
                    polling: false,
                }
            }
        }
    }

    /// Whether the wait has to ask for the exit itself, at every tick.
    pub(super) fn polling(&self) -> bool {
        self.polling
    }

    /// The child's exit status, if it can be taken now.
    ///
    /// Nothing is asked until `streams_ended`, whether a helper watches or
    /// the wait polls, and while a helper watches not until the exit is
    /// reported either. So the status is taken only when it completes the
    /// wait: until then the unreaped child keeps its pid, and its group's,
    /// from meaning anything else to a later kill, and the reap is the last
    /// thing done to the child however the wait learns of its exit. The
    /// helper is joined first, so nothing of ours still names the pid once it
    /// is reaped. A report with no status behind it turns the wait to
    /// polling, which asks at every tick from then on.
    ///
    /// A `try_wait` that fails is likewise seen only once the streams have
    /// ended. Until then the wait's deadline is what bounds it: where the
    /// status cannot be had and a stream stays open, the wait ends at its
    /// deadline, with the deadline's error.
    pub(super) fn try_status(
        &mut self,
        child: &mut Child,
        events: &WaitEvents,
        streams_ended: bool,
    ) -> std::io::Result<Option<std::process::ExitStatus>> {
        if !streams_ended {
            return Ok(None);
        }
        if !self.polling {
            if !events.ended(WaitPart::Exit) {
                return Ok(None);
            }
            self.join();
        }
        let status = child.try_wait()?;
        self.polling |= status.is_none();
        Ok(status)
    }

    /// Waits for the helper to end, which it does once the child has exited:
    /// after its report, or after the child was killed.
    pub(super) fn join(&mut self) {
        if let Some(helper) = self.helper.take() {
            let _ = helper.join();
        }
    }
}

/// A thread that blocks until `pid`, a child of this process, has exited, and
/// leaves it unreaped. Whatever ends its wait (a status, no such child, any
/// other error) is the same report: the child may have exited.
#[cfg(unix)]
fn watch_exit(pid: u32, signals: &WaitSignals) -> Option<JoinHandle<()>> {
    use rustix::process::{Pid, WaitId, WaitIdOptions, waitid};
    let pid = Pid::from_raw(i32::try_from(pid).ok()?)?;
    let peek = WaitIdOptions::EXITED | WaitIdOptions::NOWAIT;
    signals
        .start(WaitPart::Exit, move || {
            while matches!(waitid(WaitId::Pid(pid), peek), Err(rustix::io::Errno::INTR)) {}
        })
        .ok()
}

#[cfg(not(unix))]
fn watch_exit(_pid: u32, _signals: &WaitSignals) -> Option<JoinHandle<()>> {
    None
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

    /// `/bin/sh` running `script`, whose `$1` is the next argument added.
    pub(crate) fn shell(script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script, "fixture"]);
        command
    }

    /// The line a fixture's shell wrote to `path` (`echo $$ > "$1"`), once it
    /// is whole.
    pub(crate) fn written_line(path: &Path) -> String {
        let started = Instant::now();
        loop {
            if let Ok(line) = std::fs::read_to_string(path)
                && line.ends_with('\n')
            {
                return line.trim().to_owned();
            }
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "{} was never written",
                path.display()
            );
            thread::sleep(Duration::from_millis(2));
        }
    }

    /// Whether this process still has the child `pid` to wait for, running or
    /// exited: a reaped child is no longer one. Looking leaves it as it was.
    pub(crate) fn still_a_child(pid: &str) -> bool {
        child_state(pid) != ChildState::Reaped
    }

    #[derive(Debug, PartialEq, Eq)]
    pub(crate) enum ChildState {
        Running,
        /// Exited and not yet waited for: its process ID is still its own.
        Exited,
        Reaped,
    }

    /// What `pid` is to this process, which the look leaves unchanged.
    pub(crate) fn child_state(pid: &str) -> ChildState {
        use rustix::process::{Pid, WaitId, WaitIdOptions, waitid};
        let pid = Pid::from_raw(pid.trim().parse().unwrap()).unwrap();
        match waitid(
            WaitId::Pid(pid),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
        ) {
            Ok(None) => ChildState::Running,
            Ok(Some(_)) => ChildState::Exited,
            Err(rustix::io::Errno::CHILD) => ChildState::Reaped,
            Err(error) => panic!("waitid({pid:?}): {error}"),
        }
    }
}
