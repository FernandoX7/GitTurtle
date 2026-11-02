//! GitTurtle's Linux PDF helper process: its entry, self-confinement and
//! seccomp allow-list.
//!
//! The app binary becomes the helper only when started with exactly
//! `--gitturtle-pdf-helper <protocol version> <parent pid>`: [`run_if_requested`]
//! is the first statement of `main`. The helper confines itself before it
//! reads a byte, then serves one request with
//! [`gitturtle_preview::pdf_helper::serve`] and exits; hayro never runs in the
//! app process. The contract is section 6 of the
//! [architecture review](../../../docs/development/2026-11-02-linux-pdf-pages-architecture.md).
//!
//! The module depends only on std, `libc`, `anyhow` and `gitturtle-preview`:
//! `tests/pdf_helper.rs` compiles it as its own module to drive the filter and
//! the real binary.

use std::ffi::OsStr;
use std::fs::File;
use std::hash::RandomState;
use std::io;
use std::mem::ManuallyDrop;
use std::os::fd::FromRawFd;
use std::os::unix::ffi::OsStrExt;

use gitturtle_preview::pdf_helper::{PROTOCOL_VERSION, serve};

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
