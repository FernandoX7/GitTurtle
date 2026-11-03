//! GitTurtle's Linux PDF helper process (its entry, self-confinement and
//! seccomp allow-list) and the client that runs it.
//!
//! The app binary becomes the helper only when started with exactly
//! `--gitturtle-pdf-helper <protocol version> <parent pid>`: [`run_if_requested`]
//! is the first statement of `main`. The helper confines itself before it
//! reads a byte, then serves one request with
//! [`gitturtle_preview::pdf_helper::serve`] and exits; hayro never runs in the
//! app process. [`Client::render`] starts one helper per page from
//! `/proc/self/exe`, at most two across the app, and kills and reaps it on the
//! calling thread at the deadline, when the caller's check fails, or when its
//! reply exceeds the request's cap. [`Runner`] does that for every request:
//! the image client (`crate::image_helper`) shares it, with the same helper,
//! deadline and two slots. The contract is section 6 of the
//! [architecture review](../../../docs/development/2026-11-02-linux-pdf-pages-architecture.md).
//!
//! The module depends only on std, `libc`, `anyhow` and `gitturtle-preview`:
//! `tests/pdf_helper.rs` compiles it as its own module to drive the filter and
//! the real binary.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::File;
use std::hash::RandomState;
use std::io::{self, Read, Write};
use std::mem::ManuallyDrop;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use gitturtle_preview::pdf_helper::{
    ErrorKind, MAX_REPLY_BYTES, PROTOCOL_VERSION, PageReply, encode_request, read_reply, serve,
};

/// The first argument that makes this binary the helper.
pub(crate) const HELPER_FLAG: &str = "--gitturtle-pdf-helper";

/// The helper's fixed exit codes, besides 0 once the reply is written. Every
/// code but [`exit::REPLY`] means the helper stopped before reading input.
pub(crate) mod exit {
    /// The version or parent pid argument is malformed or another version.
    pub(crate) const ARGUMENTS: i32 = 120;
    /// The parent-death signal is unset, or the parent is not the one named.
    pub(crate) const PARENT: i32 = 121;
    /// A process setting, descriptor or resource limit could not be applied.
    pub(crate) const SETUP: i32 = 122;
    /// seccomp is unavailable or refused the filter: confinement unavailable.
    pub(crate) const CONFINEMENT: i32 = 123;
    /// Writing the reply failed.
    pub(crate) const REPLY: i32 = 124;
}

const MIB: u64 = 1024 * 1024;
/// The helper's resource limits as (resource, soft, hard). A limit already
/// lower stays: the helper only ever tightens one.
const LIMITS: [(libc::__rlimit_resource_t, u64, u64); 8] = [
    (libc::RLIMIT_CORE, 0, 0),
    (libc::RLIMIT_FSIZE, 0, 0),
    (libc::RLIMIT_NPROC, 0, 0),
    (libc::RLIMIT_NOFILE, 3, 3),
    // `RLIMIT_DATA` does not count the stack.
    (libc::RLIMIT_STACK, 8 * MIB, 8 * MIB),
    // The client stops the helper at 10 s of wall-clock time first.
    (libc::RLIMIT_CPU, 10, 11),
    (libc::RLIMIT_DATA, 1024 * MIB, 1024 * MIB),
    // A backstop above the address space the helper starts with.
    (libc::RLIMIT_AS, 2048 * MIB, 2048 * MIB),
];
/// The helper's own wall-clock backstop, after the client's 10 s kill and the
/// 11 s CPU limit.
const ALARM_SECONDS: u32 = 12;

/// Runs this process as the PDF helper when it was started with exactly
/// [`HELPER_FLAG`], the protocol version and the parent's pid, and never
/// returns then. Any other arguments return at once, so a repository path is
/// never taken for a helper request.
pub(crate) fn run_if_requested() {
    let mut arguments = std::env::args_os();
    if arguments.len() != 4 || arguments.nth(1).as_deref() != Some(OsStr::new(HELPER_FLAG)) {
        return;
    }
    let version = arguments.next();
    let parent = arguments.next().as_deref().and_then(parent_pid);
    let code = match parent {
        Some(parent) if version.as_deref() == Some(OsStr::new(&PROTOCOL_VERSION.to_string())) => {
            run_helper(parent)
        }
        _ => exit::ARGUMENTS,
    };
    // SAFETY: ends the process without destructors, which may make calls the
    // filter refuses.
    unsafe { libc::_exit(code) }
}

/// ASCII decimal without a sign or leading zero, in 1..=`i32::MAX`.
fn parent_pid(argument: &OsStr) -> Option<libc::pid_t> {
    let digits = argument.as_bytes();
    if digits.is_empty()
        || digits.len() > 10
        || digits[0] == b'0'
        || !digits.iter().all(u8::is_ascii_digit)
    {
        return None;
    }
    let value: u64 = std::str::from_utf8(digits).ok()?.parse().ok()?;
    libc::pid_t::try_from(value).ok()
}

fn run_helper(parent: libc::pid_t) -> i32 {
    if let Err(code) = confine(parent) {
        return code;
    }
    // (k) The request comes only from fd 0 and the reply goes only to fd 1;
    // `serve` reads the header, then the validated body, with plain reads.
    // SAFETY: fds 0 and 1 stay open for the life of the process and are
    // never closed through these handles.
    let (input, output) = unsafe {
        (
            ManuallyDrop::new(File::from_raw_fd(0)),
            ManuallyDrop::new(File::from_raw_fd(1)),
        )
    };
    match serve(&*input, &*output) {
        Ok(()) => 0,
        Err(_) => exit::REPLY,
    }
}

/// Section 6's steps (a), (e), (b) to (d), then (f) to (j), before the first
/// input byte. Every failure is a fixed exit code; only the OOM score is
/// best effort.
fn confine(parent: libc::pid_t) -> Result<(), i32> {
    // SAFETY: (a) a process flag and a query, with no pointers.
    unsafe {
        // Die with the spawning thread, and only for the parent named.
        if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) != 0
            || libc::getppid() != parent
        {
            return Err(exit::PARENT);
        }
    }
    // (e) The kernel picks the helper first when memory runs out. It comes
    // before (b): once the process is not dumpable, the file belongs to root
    // and the write fails.
    let _ = std::fs::write("/proc/self/oom_score_adj", "1000");
    // SAFETY (this block): process settings on the single-threaded helper,
    // with valid pointers to locals and static strings.
    unsafe {
        // (b) No core file or ptrace by the same user; a recognizable name.
        if libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) != 0
            || libc::prctl(libc::PR_SET_NAME, c"gitturtle-pdf".as_ptr(), 0, 0, 0) != 0
        {
            return Err(exit::SETUP);
        }
        // (c) Ignored dispositions survive exec: the time limits must kill.
        for (signal, action) in [
            (libc::SIGALRM, libc::SIG_DFL),
            (libc::SIGXCPU, libc::SIG_DFL),
            (libc::SIGXFSZ, libc::SIG_IGN),
        ] {
            if libc::signal(signal, action) == libc::SIG_ERR {
                return Err(exit::SETUP);
            }
        }
        let mut unblocked: libc::sigset_t = std::mem::zeroed();
        if libc::sigemptyset(&mut unblocked) != 0
            || libc::sigprocmask(libc::SIG_SETMASK, &unblocked, std::ptr::null_mut()) != 0
        {
            return Err(exit::SETUP);
        }
        // (d)
        libc::alarm(ALARM_SECONDS);
    }
    close_inherited()?;
    for (resource, soft, hard) in LIMITS {
        tighten(resource, soft, hard)?;
    }
    prepare_runtime();
    // SAFETY: (i) a process flag with no pointers.
    if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
        return Err(exit::SETUP);
    }
    // (j)
    Filter::new().install().map_err(|_| exit::CONFINEMENT)
}

/// (f) Closes every descriptor above 2 the helper inherited.
fn close_inherited() -> Result<(), i32> {
    // SAFETY: the raw call, since glibc's wrapper needs 2.34; nothing in this
    // process uses a descriptor above 2.
    if unsafe { libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 0u32) } == 0 {
        return Ok(());
    }
    if io::Error::last_os_error().raw_os_error() != Some(libc::ENOSYS) {
        return Err(exit::SETUP);
    }
    let listed: Vec<libc::c_int> = std::fs::read_dir("/proc/self/fd")
        .map_err(|_| exit::SETUP)?
        .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse().ok())
        .filter(|&fd| fd > 2)
        .collect();
    for fd in listed {
        // SAFETY: as above; the listing's own descriptor is already closed.
        unsafe { libc::close(fd) };
    }
    Ok(())
}

/// (g) Lowers `resource` to `soft` and `hard`, keeping a lower current value.
fn tighten(resource: libc::__rlimit_resource_t, soft: u64, hard: u64) -> Result<(), i32> {
    let mut current = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: reads into a local.
    if unsafe { libc::getrlimit(resource, &mut current) } != 0 {
        return Err(exit::SETUP);
    }
    let lowered_hard = hard.min(current.rlim_max);
    let lowered_soft = soft.min(current.rlim_cur).min(lowered_hard);
    let limit = libc::rlimit {
        rlim_cur: lowered_soft,
        // Soft equals hard except where the targets differ (CPU time).
        rlim_max: if soft == hard {
            lowered_soft
        } else {
            lowered_hard
        },
    };
    // SAFETY: a local limit no higher than the current hard limit.
    if unsafe { libc::setrlimit(resource, &limit) } != 0 {
        return Err(exit::SETUP);
    }
    Ok(())
}

/// (h) Sets up, before the filter, what the runtime would otherwise set up
/// lazily with calls the filter refuses.
fn prepare_runtime() {
    // A panic message would go to fd 2; hayro's panics become error kinds.
    std::panic::set_hook(Box::new(|_| {}));
    let _ = RandomState::new();
    #[cfg(target_arch = "aarch64")]
    let _ = std::arch::is_aarch64_feature_detected!("neon");
    #[cfg(target_arch = "x86_64")]
    let _ = std::arch::is_x86_feature_detected!("avx2");
}

/// seccomp's `AUDIT_ARCH_*` value for this build's architecture; any other
/// is killed. x32 calls carry the x86-64 value with bit 30 set in their
/// number, which no rule matches.
#[cfg(target_arch = "aarch64")]
const AUDIT_ARCH: u32 = 0xC000_00B7;
#[cfg(target_arch = "x86_64")]
const AUDIT_ARCH: u32 = 0xC000_003E;

/// Offsets into the kernel's `struct seccomp_data`.
const SYSCALL_NUMBER: u32 = 0;
const SYSCALL_ARCH: u32 = 4;

/// The low 32-bit word of argument `index`; both architectures are little-endian.
const fn argument(index: u32) -> u32 {
    16 + 8 * index
}

const LOAD: u16 = (libc::BPF_LD | libc::BPF_W | libc::BPF_ABS) as u16;
const RETURN: u16 = (libc::BPF_RET | libc::BPF_K) as u16;
const EQUALS: u16 = (libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K) as u16;
const HAS_BITS: u16 = (libc::BPF_JMP | libc::BPF_JSET | libc::BPF_K) as u16;
const JUMP: u16 = (libc::BPF_JMP | libc::BPF_JA) as u16;

/// A condition on one argument of an allowed call.
#[derive(Clone, Copy)]
enum Check {
    /// The argument is one of these values.
    OneOf(u32, &'static [u32]),
    /// The argument is the confined process's own pid, set by [`Filter::install`].
    OwnPid(u32),
    /// The argument has none of these bits.
    Without(u32, u32),
    /// The argument has this bit.
    With(u32, u32),
}

impl Check {
    fn length(self) -> usize {
        match self {
            Self::OneOf(_, values) => values.len() + 2,
            Self::OwnPid(_) | Self::Without(..) | Self::With(..) => 2,
        }
    }
}

const PROT_EXEC: u32 = libc::PROT_EXEC as u32;
const MAP_ANONYMOUS: u32 = libc::MAP_ANONYMOUS as u32;
const SIGABRT: u32 = libc::SIGABRT as u32;

/// Every call the confined helper may make; anything else kills the process.
/// It reads its request on fd 0, writes the reply on fd 1 (fd 2 is
/// `/dev/null`, where an allocation failure reports itself), allocates
/// memory that is never executable, and can only abort itself.
const ALLOWED: &[(libc::c_long, &[Check])] = &[
    (libc::SYS_read, &[Check::OneOf(0, &[0])]),
    (libc::SYS_write, &[Check::OneOf(0, &[1, 2])]),
    (libc::SYS_close, &[]),
    (
        libc::SYS_mmap,
        &[Check::Without(2, PROT_EXEC), Check::With(3, MAP_ANONYMOUS)],
    ),
    (libc::SYS_mprotect, &[Check::Without(2, PROT_EXEC)]),
    (libc::SYS_munmap, &[]),
    (libc::SYS_mremap, &[]),
    (libc::SYS_brk, &[]),
    (libc::SYS_madvise, &[]),
    (libc::SYS_futex, &[]),
    (libc::SYS_getrandom, &[]),
    (libc::SYS_clock_gettime, &[]),
    (libc::SYS_rt_sigreturn, &[]),
    (libc::SYS_rt_sigprocmask, &[]),
    (libc::SYS_sigaltstack, &[]),
    (libc::SYS_restart_syscall, &[]),
    // Rust's stack-overflow handler and `abort` reset these three.
    (
        libc::SYS_rt_sigaction,
        &[Check::OneOf(
            0,
            &[SIGABRT, libc::SIGBUS as u32, libc::SIGSEGV as u32],
        )],
    ),
    (libc::SYS_getpid, &[]),
    (libc::SYS_gettid, &[]),
    (
        libc::SYS_tgkill,
        &[Check::OwnPid(0), Check::OneOf(2, &[SIGABRT])],
    ),
    (libc::SYS_exit, &[]),
    (libc::SYS_exit_group, &[]),
];

/// The helper's seccomp program: the architecture check, then [`ALLOWED`].
pub(crate) struct Filter {
    program: Vec<libc::sock_filter>,
    /// Instructions that compare with the installing process's pid.
    own_pid: Vec<usize>,
    length: u16,
}

impl Filter {
    pub(crate) fn new() -> Self {
        let mut filter = Self {
            program: Vec::new(),
            own_pid: Vec::new(),
            length: 0,
        };
        filter.push(LOAD, SYSCALL_ARCH, 0, 0);
        filter.push(EQUALS, AUDIT_ARCH, 1, 0);
        filter.push(RETURN, libc::SECCOMP_RET_KILL_PROCESS, 0, 0);
        filter.push(LOAD, SYSCALL_NUMBER, 0, 0);
        for &(number, checks) in ALLOWED {
            filter.rule(number, checks);
        }
        filter.push(RETURN, libc::SECCOMP_RET_KILL_PROCESS, 0, 0);
        filter.length = u16::try_from(filter.program.len()).expect("the filter is short");
        filter
    }

    fn push(&mut self, code: u16, k: u32, jt: u8, jf: u8) {
        self.program.push(libc::sock_filter { code, jt, jf, k });
    }

    /// `number` passes when every check passes; the rule ends in allow, then
    /// kill, so the accumulator never needs the syscall number again.
    fn rule(&mut self, number: libc::c_long, checks: &[Check]) {
        let length = checks.iter().map(|check| check.length()).sum::<usize>() + 2;
        let skip = u8::try_from(length).expect("a rule is short");
        self.push(EQUALS, number as u32, 0, skip);
        let kill = self.program.len() + length - 1;
        let to_kill = |at: usize| u8::try_from(kill - at - 1).expect("a rule is short");
        for &check in checks {
            match check {
                Check::OneOf(index, values) => {
                    self.push(LOAD, argument(index), 0, 0);
                    for (position, &value) in values.iter().enumerate() {
                        // A match skips the remaining values and the jump to kill.
                        let past = u8::try_from(values.len() - position).expect("few values");
                        self.push(EQUALS, value, past, 0);
                    }
                    let at = self.program.len();
                    self.push(JUMP, (kill - at - 1) as u32, 0, 0);
                }
                Check::OwnPid(index) => {
                    self.push(LOAD, argument(index), 0, 0);
                    let at = self.program.len();
                    self.own_pid.push(at);
                    self.push(EQUALS, 0, 0, to_kill(at));
                }
                Check::Without(index, bits) => {
                    self.push(LOAD, argument(index), 0, 0);
                    let at = self.program.len();
                    self.push(HAS_BITS, bits, to_kill(at), 0);
                }
                Check::With(index, bits) => {
                    self.push(LOAD, argument(index), 0, 0);
                    let at = self.program.len();
                    self.push(HAS_BITS, bits, 0, to_kill(at));
                }
            }
        }
        self.push(RETURN, libc::SECCOMP_RET_ALLOW, 0, 0);
        self.push(RETURN, libc::SECCOMP_RET_KILL_PROCESS, 0, 0);
    }

    /// Confines every thread of this process (`SECCOMP_FILTER_FLAG_TSYNC`),
    /// with its own pid as the only process `tgkill` may signal. Needs
    /// `PR_SET_NO_NEW_PRIVS` first. Allocates nothing, so a child forked from
    /// a multithreaded process may call it.
    pub(crate) fn install(&mut self) -> io::Result<()> {
        // SAFETY: getpid has no preconditions.
        let pid = unsafe { libc::getpid() } as u32;
        for &at in &self.own_pid {
            self.program[at].k = pid;
        }
        let program = libc::sock_fprog {
            len: self.length,
            filter: self.program.as_mut_ptr(),
        };
        // SAFETY: `program` points at `length` initialized instructions that
        // outlive the call; the kernel copies them.
        let result = unsafe {
            libc::syscall(
                libc::SYS_seccomp,
                libc::SECCOMP_SET_MODE_FILTER,
                libc::SECCOMP_FILTER_FLAG_TSYNC,
                &program as *const libc::sock_fprog,
            )
        };
        match result {
            0 => Ok(()),
            // TSYNC names a thread it could not confine.
            1.. => Err(io::Error::from_raw_os_error(libc::EBUSY)),
            _ => Err(io::Error::last_os_error()),
        }
    }
}

/// The wall-clock time a helper gets for one page, from its start.
pub(crate) const DEADLINE: Duration = Duration::from_secs(10);
/// The longest the client goes without running the caller's check.
const TICK: Duration = Duration::from_millis(25);
/// Helpers running at once across the app: requests come from each window's
/// reader and the shared page lane.
pub(crate) static HELPERS: Slots = Slots::new(2);

/// Why a page request returned no page. Its wording is fixed: nothing the
/// helper sends reaches it, only a count the client has validated.
#[derive(Debug)]
pub(crate) enum Failure {
    /// The caller's check failed; a helper that had started was killed and
    /// reaped first.
    Cancelled(anyhow::Error),
    /// The input exceeds a bound; no helper started.
    Refused(anyhow::Error),
    /// The helper reported a document error for zero-based `page`.
    Document { kind: ErrorKind, page: usize },
    /// The helper ran past the deadline, its CPU limit or its own alarm.
    TimedOut,
    /// The helper died on `signal`, stopped in the renderer or exited without
    /// a valid reply. The signal is for tests and diagnostics, never wording.
    Stopped {
        #[allow(
            dead_code,
            reason = "never worded; only tests/pdf_helper.rs reads which signal ended the helper"
        )]
        signal: Option<i32>,
    },
    /// The reply was malformed, out of bounds, short, larger than the cap or
    /// followed by more bytes.
    Unreadable,
    /// The helper could not be started or could not confine itself.
    CouldNotStart,
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sentence = match self {
            Self::Cancelled(error) | Self::Refused(error) => return write!(formatter, "{error}"),
            Self::Document {
                kind: ErrorKind::PageOutOfRange { page_count },
                page,
            } => {
                let pages = if *page_count == 1 { "page" } else { "pages" };
                return write!(
                    formatter,
                    "PDF page {} is outside this document's {page_count} {pages}",
                    page + 1
                );
            }
            // The other platforms' wording for the same document errors.
            Self::Document { kind, .. } => match kind {
                ErrorKind::InputTooLarge => "PDF exceeds the 32 MiB input limit",
                ErrorKind::NotPdf => "Unrecognized PDF header",
                ErrorKind::PageBound => "PDF page exceeds the 100,000-page navigation bound",
                ErrorKind::Corrupt => "Cannot read PDF; corrupt or unsupported document",
                ErrorKind::Locked => {
                    "PDF is password protected; use explicit external inspection to unlock it"
                }
                ErrorKind::NoPages => "PDF contains no pages",
                ErrorKind::TooManyPages => {
                    "PDF page count exceeds the 100,000-page navigation bound"
                }
                ErrorKind::InvalidPageSize => "Invalid PDF page dimensions",
                ErrorKind::PageEdgeLimit => "Image dimensions exceed the 32768-pixel edge limit",
                ErrorKind::PagePixelLimit => "Image exceeds the 32-million-pixel limit",
                ErrorKind::Request | ErrorKind::PageOutOfRange { .. } => {
                    "The PDF renderer returned an unreadable result."
                }
                ErrorKind::RendererFailed => "The PDF renderer stopped on this page.",
            },
            Self::TimedOut => "The PDF page took longer than 10 seconds and was stopped.",
            Self::Stopped { .. } => "The PDF renderer stopped on this page.",
            Self::Unreadable => "The PDF renderer returned an unreadable result.",
            Self::CouldNotStart => "The PDF renderer could not start.",
        };
        formatter.write_str(sentence)
    }
}

impl std::error::Error for Failure {}

/// A limit on helpers running at once. A waiting request keeps running its
/// check, so cancelling it never waits for a slot.
pub(crate) struct Slots {
    running: Mutex<usize>,
    freed: Condvar,
    limit: usize,
    /// [`Slots::waiting`].
    #[cfg(test)]
    waiting: AtomicUsize,
}

impl Slots {
    pub(crate) const fn new(limit: usize) -> Self {
        Self {
            running: Mutex::new(0),
            freed: Condvar::new(),
            limit,
            #[cfg(test)]
            waiting: AtomicUsize::new(0),
        }
    }

    fn acquire(&self, check: &dyn Fn() -> anyhow::Result<()>) -> anyhow::Result<Slot<'_>> {
        #[cfg(test)]
        let mut waiting = None;
        loop {
            check()?;
            let running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
            let (mut running, _) = self
                .freed
                .wait_timeout_while(running, TICK, |running| *running >= self.limit)
                .unwrap_or_else(PoisonError::into_inner);
            if *running < self.limit {
                *running += 1;
                return Ok(Slot(self));
            }
            #[cfg(test)]
            waiting.get_or_insert_with(|| Waiting::count(&self.waiting));
        }
    }

    /// Requests that found every slot taken for a whole tick and have since
    /// neither taken a slot nor been cancelled, so tests observe a waiting
    /// request rather than sleep while it waits.
    #[cfg(test)]
    #[allow(
        dead_code,
        reason = "tests/image_helper.rs observes it; the app's own tests do not"
    )]
    pub(crate) fn waiting(&self) -> usize {
        self.waiting.load(Ordering::SeqCst)
    }
}

/// One request counted in [`Slots::waiting`] until it leaves `acquire`.
#[cfg(test)]
struct Waiting<'a>(&'a AtomicUsize);

#[cfg(test)]
impl<'a> Waiting<'a> {
    fn count(waiting: &'a AtomicUsize) -> Self {
        waiting.fetch_add(1, Ordering::SeqCst);
        Self(waiting)
    }
}

#[cfg(test)]
impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

struct Slot<'a>(&'a Slots);

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        *self
            .0
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner) -= 1;
        self.0.freed.notify_one();
    }
}

/// The program and arguments that start a helper.
pub(crate) struct HelperCommand {
    program: OsString,
    arguments: Vec<OsString>,
}

impl HelperCommand {
    /// This running executable as the helper of this process, even after a
    /// package upgrade replaced its file.
    pub(crate) fn current() -> Self {
        Self {
            program: "/proc/self/exe".into(),
            arguments: vec![
                HELPER_FLAG.into(),
                PROTOCOL_VERSION.to_string().into(),
                std::process::id().to_string().into(),
            ],
        }
    }

    /// Any program as the helper: the real binary or a shell fake.
    #[cfg(test)]
    pub(crate) fn new<A: Into<OsString>>(
        program: impl Into<OsString>,
        arguments: impl IntoIterator<Item = A>,
    ) -> Self {
        Self {
            program: program.into(),
            arguments: arguments.into_iter().map(Into::into).collect(),
        }
    }

    /// The program, then its arguments.
    #[cfg(test)]
    pub(crate) fn argv(&self) -> Vec<&OsStr> {
        std::iter::once(self.program.as_os_str())
            .chain(self.arguments.iter().map(OsString::as_os_str))
            .collect()
    }

    fn spawn(&self) -> io::Result<Child> {
        // No `pre_exec`, uid, gid or groups, so std keeps `posix_spawn` in the
        // multithreaded app.
        Command::new(&self.program)
            .args(&self.arguments)
            .env_clear()
            .current_dir("/")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
    }
}

/// Runs one request in a new confined helper, for every frame: the helper
/// program, its deadline and the slots it shares. [`Client`] renders PDF
/// pages through it and `crate::image_helper::Client` decodes images.
pub(crate) struct Runner<'a> {
    command: HelperCommand,
    deadline: Duration,
    slots: &'a Slots,
}

impl Runner<'static> {
    /// This executable as the helper, with the 10 s deadline and the app's
    /// two helper slots.
    pub(crate) fn new() -> Self {
        Self {
            command: HelperCommand::current(),
            deadline: DEADLINE,
            slots: &HELPERS,
        }
    }
}

impl<'a> Runner<'a> {
    /// The helper command, deadline and slots a runner uses.
    #[cfg(test)]
    pub(crate) fn parts(&self) -> (&HelperCommand, Duration, &'a Slots) {
        (&self.command, self.deadline, self.slots)
    }

    /// A runner for tests: another helper, a shorter deadline, own slots.
    #[cfg(test)]
    pub(crate) fn with(command: HelperCommand, deadline: Duration, slots: &'a Slots) -> Self {
        Self {
            command,
            deadline,
            slots,
        }
    }

    /// Writes `request` to a new helper and collects at most `cap` reply
    /// bytes until it exits. Blocks the calling thread, which starts, feeds,
    /// waits for, kills and reaps the helper: the parent-death signal follows
    /// the thread that spawned it. `check` runs at least every 25 ms, also
    /// while waiting for a slot; the deadline counts from the helper's start.
    pub(crate) fn run(
        &self,
        request: [&[u8]; 2],
        cap: usize,
        check: &dyn Fn() -> anyhow::Result<()>,
    ) -> Outcome {
        self.run_timed(request, cap, check).0
    }

    /// [`Self::run`], and how long its helper held the slot: from the
    /// helper's start until it ended and was reaped, or zero when none
    /// started, so the wait for a slot never counts. A Markdown document
    /// totals its images' times (`crate::image_helper::DocumentImages`).
    pub(crate) fn run_timed(
        &self,
        request: [&[u8]; 2],
        cap: usize,
        check: &dyn Fn() -> anyhow::Result<()>,
    ) -> (Outcome, Duration) {
        let _slot = match self.slots.acquire(check) {
            Ok(slot) => slot,
            Err(error) => return (Outcome::Cancelled(error), Duration::ZERO),
        };
        if let Err(error) = check() {
            return (Outcome::Cancelled(error), Duration::ZERO);
        }
        let Ok(child) = self.command.spawn() else {
            return (Outcome::CouldNotStart, Duration::ZERO);
        };
        let started = Instant::now();
        let Some(mut running) = Running::new(child) else {
            return (Outcome::CouldNotStart, started.elapsed());
        };
        let (ending, reply) = running.exchange(request, cap, started + self.deadline, check);
        (Outcome::of(ending, reply), started.elapsed())
    }

    /// The wall-clock time one helper gets, from its start.
    pub(crate) fn deadline(&self) -> Duration {
        self.deadline
    }
}

/// How a helper run ended, the same for every frame: a reply counts only
/// after exit status 0, and the frame's own reader decides the rest.
pub(crate) enum Outcome {
    /// The helper exited with status 0 after writing this, at most the cap.
    Replied(Vec<u8>),
    /// The caller's check failed; a helper that had started was killed and
    /// reaped first.
    Cancelled(anyhow::Error),
    /// The helper ran past the deadline, its CPU limit or its own alarm.
    TimedOut,
    /// The helper died on `signal`, or exited with a code other than 0 and
    /// the setup codes, or could not be waited for.
    Stopped { signal: Option<i32> },
    /// The reply grew past the request's cap; the helper was killed.
    Oversized,
    /// The helper could not be started or could not confine itself.
    CouldNotStart,
}

impl Outcome {
    /// Signals and exit codes as section 6 maps them.
    fn of(ending: Ending, reply: Vec<u8>) -> Self {
        let status = match ending {
            Ending::Killed(Kill::Deadline) => return Self::TimedOut,
            Ending::Killed(Kill::Cancelled(error)) => return Self::Cancelled(error),
            Ending::Killed(Kill::Oversized) => return Self::Oversized,
            Ending::Killed(Kill::Unwaitable) => return Self::Stopped { signal: None },
            Ending::Exited(status) => status,
        };
        if let Some(signal) = status.signal() {
            return match signal {
                libc::SIGXCPU | libc::SIGALRM => Self::TimedOut,
                _ => Self::Stopped {
                    signal: Some(signal),
                },
            };
        }
        match status.code() {
            Some(0) => Self::Replied(reply),
            Some(code) if (exit::ARGUMENTS..=exit::CONFINEMENT).contains(&code) => {
                Self::CouldNotStart
            }
            _ => Self::Stopped { signal: None },
        }
    }

    /// The outcome of a helper that ended with `status` after writing
    /// `reply`, so tests can cover the signals that would make a fake helper
    /// dump core.
    #[cfg(test)]
    pub(crate) fn exited(status: ExitStatus, reply: &[u8]) -> Self {
        Self::of(Ending::Exited(status), reply.to_vec())
    }
}

/// Renders PDF pages in confined helper processes, one per request.
pub(crate) struct Client<'a> {
    runner: Runner<'a>,
}

impl Client<'static> {
    /// This executable as the helper, with the 10 s deadline and the app's
    /// two helper slots.
    pub(crate) fn new() -> Self {
        Self {
            runner: Runner::new(),
        }
    }
}

impl<'a> Client<'a> {
    /// The helper command, deadline and slots a client uses.
    #[cfg(test)]
    pub(crate) fn parts(&self) -> (&HelperCommand, Duration, &'a Slots) {
        self.runner.parts()
    }

    /// A client for tests: another helper, a shorter deadline, own slots.
    #[cfg(test)]
    pub(crate) fn with(command: HelperCommand, deadline: Duration, slots: &'a Slots) -> Self {
        Self {
            runner: Runner::with(command, deadline, slots),
        }
    }

    /// Zero-based `page` of `pdf` and its text, from a new helper through
    /// [`Runner::run`], with the page frame's cap.
    pub(crate) fn render(
        &self,
        pdf: &[u8],
        page: usize,
        check: &dyn Fn() -> anyhow::Result<()>,
    ) -> Result<PageReply, Failure> {
        let header = encode_request(page, pdf).map_err(Failure::Refused)?;
        classify(
            self.runner.run([&header, pdf], MAX_REPLY_BYTES, check),
            page,
        )
    }
}

/// How the exchange with a helper ended.
enum Ending {
    Exited(ExitStatus),
    /// The client killed and reaped the helper.
    Killed(Kill),
}

enum Kill {
    Deadline,
    Cancelled(anyhow::Error),
    Oversized,
    /// Waiting for the helper failed.
    Unwaitable,
}

/// A started helper. Dropping it kills and reaps the process on every path;
/// `Child::kill` does nothing once a wait has succeeded, so a reused pid is
/// never signalled.
struct Running {
    child: Child,
    input: Option<ChildStdin>,
    output: Option<ChildStdout>,
    /// Readable once the helper exits, where the kernel has pidfds (5.3).
    exited: Option<OwnedFd>,
}

impl Running {
    /// `None` when its pipes cannot be set up; dropping it reaps the child.
    fn new(mut child: Child) -> Option<Self> {
        let running = Self {
            input: child.stdin.take(),
            output: child.stdout.take(),
            exited: pidfd(child.id()),
            child,
        };
        for fd in [
            running.input.as_ref().map(AsRawFd::as_raw_fd),
            running.output.as_ref().map(AsRawFd::as_raw_fd),
        ] {
            non_blocking(fd?).ok()?;
        }
        Some(running)
    }

    /// Writes `request` and collects the whole reply, at most `cap` bytes,
    /// until the helper exits, the deadline passes or `check` fails.
    fn exchange(
        &mut self,
        request: [&[u8]; 2],
        cap: usize,
        deadline: Instant,
        check: &dyn Fn() -> anyhow::Result<()>,
    ) -> (Ending, Vec<u8>) {
        let mut reply = Vec::new();
        let mut written = 0;
        let mut reading = true;
        let mut chunk = vec![0; 64 * 1024];
        // Without a pidfd, waiting for the exit backs off from 1 ms to a tick.
        let mut backoff = Duration::from_millis(1);
        loop {
            if let Err(error) = check() {
                return (self.kill(Kill::Cancelled(error)), reply);
            }
            let now = Instant::now();
            if now >= deadline {
                return (self.kill(Kill::Deadline), reply);
            }
            let mut wait = (deadline - now).min(TICK);
            if !reading {
                // The reply is complete once the helper closes its output.
                match self.child.try_wait() {
                    Ok(Some(status)) => return (Ending::Exited(status), reply),
                    Ok(None) if self.exited.is_none() => {
                        wait = wait.min(backoff);
                        backoff = (backoff * 2).min(TICK);
                    }
                    Ok(None) => {}
                    Err(_) => return (self.kill(Kill::Unwaitable), reply),
                }
            }
            let mut polled = [
                pollfd(
                    self.output
                        .as_ref()
                        .filter(|_| reading)
                        .map(AsRawFd::as_raw_fd),
                    libc::POLLIN,
                ),
                pollfd(self.input.as_ref().map(AsRawFd::as_raw_fd), libc::POLLOUT),
                pollfd(
                    self.exited
                        .as_ref()
                        .filter(|_| !reading)
                        .map(AsRawFd::as_raw_fd),
                    libc::POLLIN,
                ),
            ];
            let millis = libc::c_int::try_from(wait.as_millis().max(1)).unwrap_or(25);
            // SAFETY: polls the entries of a local array.
            if unsafe { libc::poll(polled.as_mut_ptr(), polled.len() as libc::nfds_t, millis) } < 0
            {
                continue;
            }
            if polled[1].revents != 0 {
                self.write(request, &mut written);
            }
            if polled[0].revents != 0 && !self.read(&mut reply, cap, &mut chunk, &mut reading) {
                return (self.kill(Kill::Oversized), reply);
            }
        }
    }

    /// Writes what the pipe takes; stops writing once everything is written
    /// or the helper closed its input.
    fn write(&mut self, request: [&[u8]; 2], written: &mut usize) {
        let Some(input) = self.input.as_mut() else {
            return;
        };
        let [header, body] = request;
        loop {
            let rest = match header.get(*written..) {
                Some(rest) if !rest.is_empty() => rest,
                _ => &body[*written - header.len()..],
            };
            if rest.is_empty() {
                self.input = None;
                return;
            }
            match input.write(rest) {
                Ok(count) => *written += count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return,
                Err(_) => {
                    self.input = None;
                    return;
                }
            }
        }
    }

    /// Reads what the pipe holds into `reply`; false once it exceeds `cap`.
    fn read(
        &mut self,
        reply: &mut Vec<u8>,
        cap: usize,
        chunk: &mut [u8],
        reading: &mut bool,
    ) -> bool {
        let Some(output) = self.output.as_mut() else {
            *reading = false;
            return true;
        };
        loop {
            match output.read(chunk) {
                Ok(0) => {
                    *reading = false;
                    self.input = None;
                    return true;
                }
                Ok(count) => {
                    if reply.len() + count > cap {
                        return false;
                    }
                    reply.extend_from_slice(&chunk[..count]);
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return true,
                Err(_) => {
                    *reading = false;
                    return true;
                }
            }
        }
    }

    fn kill(&mut self, why: Kill) -> Ending {
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ending::Killed(why)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A pidfd for `pid`, a child not yet waited for, so the pid is still ours.
fn pidfd(pid: u32) -> Option<OwnedFd> {
    let pid = libc::pid_t::try_from(pid).ok()?;
    // SAFETY: opens a new descriptor; pidfds are close-on-exec.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    let fd = RawFd::try_from(fd).ok().filter(|&fd| fd >= 0)?;
    // SAFETY: the descriptor was just opened and nothing else owns it.
    Some(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn pollfd(fd: Option<RawFd>, events: libc::c_short) -> libc::pollfd {
    libc::pollfd {
        // A negative descriptor is skipped.
        fd: fd.unwrap_or(-1),
        events,
        revents: 0,
    }
}

fn non_blocking(fd: RawFd) -> io::Result<()> {
    // SAFETY: reads and sets the status flags of a pipe this process owns.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// [`classify`] for a helper that ended with `status`, so tests can cover the
/// signals that would make a fake helper dump core.
#[cfg(test)]
pub(crate) fn classify_exit(
    status: ExitStatus,
    reply: &[u8],
    page: usize,
) -> Result<PageReply, Failure> {
    classify(Outcome::exited(status, reply), page)
}

/// Success needs a valid frame for the requested page and exit status 0.
fn classify(outcome: Outcome, page: usize) -> Result<PageReply, Failure> {
    let reply = match outcome {
        Outcome::Replied(reply) => reply,
        Outcome::Cancelled(error) => return Err(Failure::Cancelled(error)),
        Outcome::TimedOut => return Err(Failure::TimedOut),
        Outcome::Stopped { signal } => return Err(Failure::Stopped { signal }),
        Outcome::Oversized => return Err(Failure::Unreadable),
        Outcome::CouldNotStart => return Err(Failure::CouldNotStart),
    };
    match read_reply(&reply[..]) {
        Ok(Ok(rendered)) if rendered.page == page => Ok(rendered),
        // A count the request contradicts is a malformed reply.
        Ok(Err(ErrorKind::PageOutOfRange { page_count })) if page_count > page => {
            Err(Failure::Unreadable)
        }
        Ok(Err(kind)) => Err(Failure::Document { kind, page }),
        Ok(Ok(_)) | Err(_) => Err(Failure::Unreadable),
    }
}
