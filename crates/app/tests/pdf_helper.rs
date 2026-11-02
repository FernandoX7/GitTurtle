//! The Linux PDF helper's confinement: its seccomp filter in forked children.
//!
//! The app crate has no library target, so this test compiles
//! `src/pdf_helper.rs` as its own module.
#![cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]

#[allow(dead_code, reason = "each test uses part of the module")]
#[path = "../src/pdf_helper.rs"]
mod pdf_helper;

mod filter {
    use std::fs::File;
    use std::io;
    use std::os::fd::AsRawFd;
    use std::ptr;

    use libc::{c_long, syscall};

    use super::pdf_helper::Filter;

    #[derive(Debug, PartialEq, Eq)]
    enum Ended {
        Exited(i32),
        Signaled(i32),
    }

    const SIGSYS: Ended = Ended::Signaled(libc::SIGSYS);

    /// A named call for a forked child to make.
    type Case = (&'static str, Box<dyn Fn()>);

    /// Builds the filter here, then forks a child that points fds 0 to 3 at
    /// `/dev/null`, runs `prepare`, sets `no_new_privs`, installs the filter,
    /// makes `call` and exits 0. The child allocates nothing.
    fn after(prepare: fn(), call: &dyn Fn()) -> Ended {
        let mut filter = Filter::new();
        let null = File::options()
            .read(true)
            .write(true)
            .open("/dev/null")
            .unwrap();
        // SAFETY: the child only makes async-signal-safe calls and writes
        // into memory it owns before it exits.
        let child = unsafe { libc::fork() };
        if child == 0 {
            // SAFETY: plain system calls on descriptors this child owns.
            unsafe {
                for fd in 0..=3 {
                    libc::dup2(null.as_raw_fd(), fd);
                }
                // No core file from the kills below.
                libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
                prepare();
                if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
                    || filter.install().is_err()
                {
                    libc::_exit(100);
                }
                call();
                libc::_exit(0)
            }
        }
        assert!(child > 0, "fork failed: {}", io::Error::last_os_error());
        let mut status = 0;
        // SAFETY: waits for this test's own child only.
        while unsafe { libc::waitpid(child, &mut status, 0) } != child {
            assert_eq!(
                io::Error::last_os_error().kind(),
                io::ErrorKind::Interrupted
            );
        }
        if libc::WIFEXITED(status) {
            Ended::Exited(libc::WEXITSTATUS(status))
        } else {
            Ended::Signaled(libc::WTERMSIG(status))
        }
    }

    fn calling(call: &dyn Fn()) -> Ended {
        after(|| {}, call)
    }

    fn pid() -> c_long {
        // SAFETY: no preconditions.
        c_long::from(unsafe { libc::getpid() })
    }

    fn tid() -> c_long {
        // SAFETY: no preconditions.
        c_long::from(unsafe { libc::gettid() })
    }

    fn sigaction_of(signal: c_long) -> impl Fn() {
        move || {
            let mut old = [0u8; 64];
            // SAFETY: queries a disposition into a buffer larger than the
            // kernel's `struct sigaction`.
            unsafe {
                syscall(
                    libc::SYS_rt_sigaction,
                    signal,
                    ptr::null::<u8>(),
                    old.as_mut_ptr(),
                    8,
                )
            };
        }
    }

    fn mmap(prot: c_long, flags: c_long, fd: c_long) -> impl Fn() {
        move || {
            // SAFETY: maps a fresh page or fails; the page is never used.
            unsafe { syscall(libc::SYS_mmap, 0, 4096, prot, flags, fd, 0) };
        }
    }

    const PAGE: c_long = 0x10000;
    const READ_WRITE: c_long = (libc::PROT_READ | libc::PROT_WRITE) as c_long;
    const ANONYMOUS: c_long = (libc::MAP_PRIVATE | libc::MAP_ANONYMOUS) as c_long;

    #[test]
    fn every_allowed_form_returns_or_exits() {
        let mut byte = [0u8; 8];
        let buffer = byte.as_mut_ptr();
        let mut word = 0u32;
        let futex = &mut word as *mut u32;
        let mut time = [0u64; 2];
        let time = time.as_mut_ptr();
        let mut stack = [0u64; 4];
        let stack = stack.as_mut_ptr();
        let mut mask = 0u64;
        let mask = &mut mask as *mut u64;
        // SAFETY (every case): each call reads or writes only the buffers above,
        // maps a page it never uses, or changes nothing (zero lengths).
        let cases: Vec<Case> = vec![
            (
                "read on fd 0",
                Box::new(move || unsafe {
                    syscall(libc::SYS_read, 0, buffer, 1);
                }),
            ),
            (
                "write on fd 1",
                Box::new(move || unsafe {
                    syscall(libc::SYS_write, 1, buffer, 1);
                }),
            ),
            (
                "write on fd 2",
                Box::new(move || unsafe {
                    syscall(libc::SYS_write, 2, buffer, 1);
                }),
            ),
            (
                "close",
                Box::new(|| unsafe {
                    syscall(libc::SYS_close, 3);
                }),
            ),
            ("anonymous mmap", Box::new(mmap(READ_WRITE, ANONYMOUS, -1))),
            (
                "mprotect",
                Box::new(|| unsafe {
                    syscall(libc::SYS_mprotect, PAGE, 0, libc::PROT_READ);
                }),
            ),
            (
                "munmap",
                Box::new(|| unsafe {
                    syscall(libc::SYS_munmap, PAGE, 0);
                }),
            ),
            (
                "mremap",
                Box::new(|| unsafe {
                    syscall(libc::SYS_mremap, PAGE, 0, 0, 0);
                }),
            ),
            (
                "brk",
                Box::new(|| unsafe {
                    syscall(libc::SYS_brk, 0);
                }),
            ),
            (
                "madvise",
                Box::new(|| unsafe {
                    syscall(libc::SYS_madvise, PAGE, 0, libc::MADV_NORMAL);
                }),
            ),
            (
                "futex",
                Box::new(move || unsafe {
                    syscall(
                        libc::SYS_futex,
                        futex,
                        libc::FUTEX_WAKE | libc::FUTEX_PRIVATE_FLAG,
                        1,
                        0,
                        0,
                        0,
                    );
                }),
            ),
            (
                "getrandom",
                Box::new(move || unsafe {
                    syscall(libc::SYS_getrandom, buffer, 1, 0);
                }),
            ),
            (
                "clock_gettime",
                Box::new(move || unsafe {
                    syscall(libc::SYS_clock_gettime, libc::CLOCK_MONOTONIC, time);
                }),
            ),
            (
                "rt_sigprocmask",
                Box::new(move || unsafe {
                    syscall(
                        libc::SYS_rt_sigprocmask,
                        libc::SIG_BLOCK,
                        ptr::null::<u64>(),
                        mask,
                        8,
                    );
                }),
            ),
            (
                "sigaltstack",
                Box::new(move || unsafe {
                    syscall(libc::SYS_sigaltstack, ptr::null::<u8>(), stack);
                }),
            ),
            (
                "restart_syscall",
                Box::new(|| unsafe {
                    syscall(libc::SYS_restart_syscall);
                }),
            ),
            (
                "rt_sigaction(SIGABRT)",
                Box::new(sigaction_of(libc::SIGABRT.into())),
            ),
            (
                "rt_sigaction(SIGBUS)",
                Box::new(sigaction_of(libc::SIGBUS.into())),
            ),
            (
                "rt_sigaction(SIGSEGV)",
                Box::new(sigaction_of(libc::SIGSEGV.into())),
            ),
            (
                "getpid",
                Box::new(|| unsafe {
                    syscall(libc::SYS_getpid);
                }),
            ),
            (
                "gettid",
                Box::new(|| unsafe {
                    syscall(libc::SYS_gettid);
                }),
            ),
            (
                "exit_group",
                Box::new(|| unsafe {
                    syscall(libc::SYS_exit_group, 0);
                }),
            ),
        ];
        for (case, call) in &cases {
            assert_eq!(calling(call), Ended::Exited(0), "{case}");
        }
        // SAFETY: ends the only thread with its own code.
        assert_eq!(
            calling(&|| unsafe {
                syscall(libc::SYS_exit, 7);
            }),
            Ended::Exited(7),
            "exit"
        );
    }

    extern "C" fn returns(_: libc::c_int) {}

    #[test]
    fn the_helper_may_abort_only_itself() {
        // SAFETY: signals this child's own thread.
        let abort = || unsafe {
            syscall(libc::SYS_tgkill, pid(), tid(), libc::SIGABRT);
        };
        assert_eq!(calling(&abort), Ended::Signaled(libc::SIGABRT));
        // With a handler set before confinement, the signal returns through
        // rt_sigreturn and the child exits normally.
        let handle_abort = || {
            // SAFETY: installs an async-signal-safe handler that does nothing.
            unsafe { libc::signal(libc::SIGABRT, returns as *const () as libc::sighandler_t) };
        };
        assert_eq!(after(handle_abort, &abort), Ended::Exited(0));
        // SAFETY: getpid has no preconditions.
        let parent = c_long::from(unsafe { libc::getpid() });
        // The parent's pid with this child's tid: a refused form even if the
        // kernel would have found no such thread.
        // SAFETY: as above.
        let at_parent = move || unsafe {
            syscall(libc::SYS_tgkill, parent, tid(), libc::SIGABRT);
        };
        assert_eq!(calling(&at_parent), SIGSYS, "tgkill on the parent");
        // SAFETY: as above; refused before any signal is sent.
        let other_signal = || unsafe {
            syscall(libc::SYS_tgkill, pid(), tid(), libc::SIGALRM);
        };
        assert_eq!(calling(&other_signal), SIGSYS, "tgkill with SIGALRM");
    }

    #[test]
    fn every_other_form_kills_with_sigsys() {
        let mut byte = [0u8; 8];
        let buffer = byte.as_mut_ptr();
        let mut old = [0u64; 2];
        let old = old.as_mut_ptr();
        let mut action = 0u32;
        let action = &mut action as *mut u32;
        let root = c"/".as_ptr();
        let missing = c"/nonexistent/gitturtle".as_ptr();
        // SAFETY (every case): each call is harmless had it been allowed: it
        // reads `/dev/null` or a query into the buffers above, or fails.
        let cases: Vec<Case> = vec![
            (
                "openat",
                Box::new(move || unsafe {
                    syscall(libc::SYS_openat, libc::AT_FDCWD, root, libc::O_RDONLY);
                }),
            ),
            (
                "socket",
                Box::new(|| unsafe {
                    syscall(libc::SYS_socket, libc::AF_UNIX, libc::SOCK_STREAM, 0);
                }),
            ),
            (
                "connect",
                Box::new(|| unsafe {
                    syscall(libc::SYS_connect, -1, 0, 0);
                }),
            ),
            (
                "kill",
                Box::new(|| unsafe {
                    syscall(libc::SYS_kill, pid(), 0);
                }),
            ),
            (
                "ptrace",
                Box::new(|| unsafe {
                    syscall(libc::SYS_ptrace, libc::PTRACE_PEEKDATA, 1, 0, 0);
                }),
            ),
            (
                "execve",
                Box::new(move || unsafe {
                    syscall(libc::SYS_execve, missing, 0, 0);
                }),
            ),
            (
                "clone3",
                Box::new(|| unsafe {
                    syscall(libc::SYS_clone3, 0, 0);
                }),
            ),
            (
                "prctl",
                Box::new(|| unsafe {
                    syscall(libc::SYS_prctl, libc::PR_GET_DUMPABLE, 0, 0, 0, 0);
                }),
            ),
            (
                "prlimit64",
                Box::new(move || unsafe {
                    syscall(libc::SYS_prlimit64, 0, libc::RLIMIT_NOFILE, 0, old);
                }),
            ),
            (
                "seccomp",
                Box::new(move || unsafe {
                    syscall(libc::SYS_seccomp, libc::SECCOMP_GET_ACTION_AVAIL, 0, action);
                }),
            ),
            (
                "read on fd 3",
                Box::new(move || unsafe {
                    syscall(libc::SYS_read, 3, buffer, 1);
                }),
            ),
            (
                "read on fd 1",
                Box::new(move || unsafe {
                    syscall(libc::SYS_read, 1, buffer, 1);
                }),
            ),
            (
                "write on fd 3",
                Box::new(move || unsafe {
                    syscall(libc::SYS_write, 3, buffer, 1);
                }),
            ),
            (
                "write on fd 0",
                Box::new(move || unsafe {
                    syscall(libc::SYS_write, 0, buffer, 1);
                }),
            ),
            (
                "executable mmap",
                Box::new(mmap(
                    c_long::from(libc::PROT_READ | libc::PROT_EXEC),
                    ANONYMOUS,
                    -1,
                )),
            ),
            (
                "file-backed mmap on fd 0",
                Box::new(mmap(
                    c_long::from(libc::PROT_READ),
                    c_long::from(libc::MAP_PRIVATE),
                    0,
                )),
            ),
            (
                "executable mprotect",
                Box::new(|| unsafe {
                    syscall(
                        libc::SYS_mprotect,
                        PAGE,
                        0,
                        libc::PROT_READ | libc::PROT_EXEC,
                    );
                }),
            ),
            (
                "rt_sigaction(SIGALRM)",
                Box::new(sigaction_of(libc::SIGALRM.into())),
            ),
            (
                "rt_sigaction(SIGSYS)",
                Box::new(sigaction_of(libc::SIGSYS.into())),
            ),
        ];
        for (case, call) in &cases {
            assert_eq!(calling(call), SIGSYS, "{case}");
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn an_x32_call_kills_with_sigsys() {
        const X32: c_long = 0x4000_0000;
        // SAFETY: getpid's x32 number; harmless had it been allowed.
        assert_eq!(
            calling(&|| unsafe {
                syscall(X32 | libc::SYS_getpid);
            }),
            SIGSYS
        );
    }
}
