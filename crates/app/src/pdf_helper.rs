//! GitTurtle's Linux PDF helper process: its seccomp allow-list.
//!
//! The contract is section 6 of the
//! [architecture review](../../../docs/development/2026-11-02-linux-pdf-pages-architecture.md).
//! The module depends only on std, `libc`, `anyhow` and `gitturtle-preview`:
//! `tests/pdf_helper.rs` compiles it as its own module to drive the filter.

use std::io;

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
