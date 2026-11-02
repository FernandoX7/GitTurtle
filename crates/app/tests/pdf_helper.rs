//! The Linux PDF helper: its seccomp filter in forked children, the real
//! binary as a confined helper, and the client against shell fakes.
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

/// PDFs built in test code.
mod fixtures {
    /// A PDF of `objects`, numbered from 1 in order; object 1 is the catalog.
    pub(crate) fn assemble(objects: &[Vec<u8>]) -> Vec<u8> {
        let mut pdf = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            pdf.extend_from_slice(object);
            pdf.extend_from_slice(b"\nendobj\n");
        }
        let xref = pdf.len();
        pdf.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    /// A stream object of `data` with the other `dictionary` entries.
    pub(crate) fn stream(dictionary: &str, data: &[u8]) -> Vec<u8> {
        let mut object =
            format!("<< {dictionary} /Length {} >>\nstream\n", data.len()).into_bytes();
        object.extend_from_slice(data);
        object.extend_from_slice(b"\nendstream");
        object
    }

    /// One 120 × 80 point page drawing `content`, with Helvetica as `/F1` and
    /// `image`, a stream object, as `/Im0`.
    pub(crate) fn page(content: &[u8], image: Option<Vec<u8>>) -> Vec<u8> {
        let image_resource = if image.is_some() {
            " /XObject << /Im0 6 0 R >>"
        } else {
            ""
        };
        let mut objects = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 80] /Resources << /Font << /F1 4 0 R >>{image_resource} >> /Contents 5 0 R >>"
            )
            .into_bytes(),
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
                .to_vec(),
            stream("", content),
        ];
        objects.extend(image);
        assemble(&objects)
    }

    /// A page whose top half is red, with "Benign" in its bottom half.
    pub(crate) fn benign() -> Vec<u8> {
        page(
            b"1 0 0 rg 0 40 120 40 re f 0 g BT /F1 12 Tf 10 10 Td (Benign) Tj ET",
            None,
        )
    }
}

/// The real binary as the helper: confinement, descriptors, parent and
/// arguments.
mod helper {
    use std::fs;
    use std::io::{self, BufRead, BufReader, Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use gitturtle_preview::pdf_helper::{PROTOCOL_VERSION, encode_request, read_reply};

    use super::fixtures;
    use super::pdf_helper::{HELPER_FLAG, exit};

    const BINARY: &str = env!("CARGO_BIN_EXE_gitturtle");

    fn version() -> String {
        PROTOCOL_VERSION.to_string()
    }

    fn own_pid() -> String {
        std::process::id().to_string()
    }

    /// The binary as a helper of this process, with no environment.
    fn helper(parent: &str) -> Command {
        let mut command = Command::new(BINARY);
        command
            .args([HELPER_FLAG, &version(), parent])
            .env_clear()
            .current_dir("/")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        command
    }

    fn field<'a>(status: &'a str, name: &str) -> &'a str {
        status
            .lines()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix(':'))
            .map(str::trim)
            .unwrap_or_else(|| panic!("no {name} in\n{status}"))
    }

    /// How many seccomp filters confine a process, from its `status`.
    fn filters(status: &str) -> u32 {
        field(status, "Seccomp_filters").parse().unwrap()
    }

    /// The filters this test thread runs under, which every process it
    /// starts inherits: none on a desktop, one under Docker's default
    /// profile (every process of a `container:` CI job) or Flatpak's.
    /// Filters belong to a thread, and the helper is forked from this one.
    fn own_filters() -> u32 {
        filters(&fs::read_to_string("/proc/thread-self/status").unwrap())
    }

    /// Whether a process whose `status` this is, started under `inherited`
    /// filters, has installed a filter of its own.
    ///
    /// The count is relative because `Seccomp: 2` only says some filter is
    /// in place: under an outer filter the helper reads 2 from its first
    /// instruction, before it names itself or confines anything, and a wait
    /// for 2 returned there on Arch CI. The helper's filter is always the
    /// last one added, so one filter more than it inherited means its own.
    fn confined(status: &str, inherited: u32) -> bool {
        field(status, "Seccomp") == "2" && filters(status) > inherited
    }

    /// `/proc/<pid>/status` once the helper has installed its own filter,
    /// the last step before it reads, on top of the `inherited` filters it
    /// started under.
    fn confined_status(pid: u32, inherited: u32) -> String {
        let started = Instant::now();
        loop {
            let status = fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
            if confined(&status, inherited) {
                return status;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the helper never confined itself:\n{status}"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Whether `pid` runs: neither reaped nor a zombie.
    fn running(pid: u32) -> bool {
        let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else {
            return false;
        };
        let state = stat[stat.rfind(')').unwrap() + 1..].trim_start();
        !state.starts_with(['Z', 'X'])
    }

    /// Waits up to 10 s for `child` to exit on its own.
    fn exit_code(child: &mut std::process::Child) -> Option<i32> {
        let started = Instant::now();
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                return status.code();
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the helper did not exit"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn a_confined_helper_round_trips_a_page() {
        round_trip_a_page(|| Ok(()), 0);
    }

    /// Flatpak, and Docker's default profile for every process of a
    /// `container:` job, start the app under a seccomp filter of their own,
    /// which the helper inherits: it reads `Seccomp: 2` before it confines
    /// itself, so its confinement is told by its own filter's count.
    #[test]
    fn a_helper_started_under_an_outer_filter_round_trips_a_page() {
        round_trip_a_page(allow_every_call, 1);
    }

    /// A process an outer filter started reads `Seccomp: 2` holding only
    /// the filters it inherited, as the helper does until it confines
    /// itself: that is not confinement, and one filter more is. The status
    /// is a real one, of a `cat` waiting on its input under
    /// [`allow_every_call`], so this does not race the helper's setup.
    #[test]
    fn only_a_filter_of_its_own_counts_as_confinement() {
        let inherited = own_filters();
        let mut command = Command::new("/bin/cat");
        command
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // SAFETY: only `allow_every_call` between fork and exec.
        unsafe {
            command.pre_exec(allow_every_call);
        }
        // `spawn` returns once `cat` has replaced the child: the filter is
        // in place and `cat` adds none.
        let mut child = command.spawn().unwrap();
        let status = fs::read_to_string(format!("/proc/{}/status", child.id())).unwrap();
        drop(child.stdin.take());
        assert_eq!(child.wait().unwrap().code(), Some(0));
        assert_eq!(field(&status, "Seccomp"), "2");
        assert_eq!(filters(&status), inherited + 1);
        assert!(
            !confined(&status, inherited + 1),
            "an outer filter alone is not the helper's own:\n{status}"
        );
        assert!(
            confined(&status, inherited),
            "one filter more than inherited is:\n{status}"
        );
    }

    /// Installs a filter that allows every call, as a sandbox does before it
    /// starts the app. Runs between fork and exec: two `prctl` calls on a
    /// program on this stack, and no allocation.
    fn allow_every_call() -> io::Result<()> {
        let mut allow = [libc::sock_filter {
            code: (libc::BPF_RET | libc::BPF_K) as u16,
            jt: 0,
            jf: 0,
            k: libc::SECCOMP_RET_ALLOW,
        }];
        let program = libc::sock_fprog {
            len: 1,
            filter: allow.as_mut_ptr(),
        };
        // SAFETY: a process flag, then a pointer to one initialized
        // instruction that outlives the call; the kernel copies it.
        let installed = unsafe {
            libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1 as libc::c_ulong, 0, 0, 0) == 0
                && libc::prctl(
                    libc::PR_SET_SECCOMP,
                    libc::SECCOMP_MODE_FILTER as libc::c_ulong,
                    &program as *const libc::sock_fprog,
                ) == 0
        };
        if installed {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// Starts the helper with every signal blocked and SIGALRM and SIGXCPU
    /// ignored, after `outer`, which adds `outer_filters` seccomp filters,
    /// has run in the child; checks each confinement step once its own
    /// filter is in place, then renders a page through it.
    fn round_trip_a_page(outer: fn() -> io::Result<()>, outer_filters: u32) {
        let inherited = own_filters() + outer_filters;
        let mut command = helper(&own_pid());
        // SAFETY: only signal calls and `outer`'s between fork and exec,
        // after std has reset the mask; the signal changes survive exec, so
        // the helper must undo them.
        unsafe {
            command.pre_exec(move || {
                let mut every: libc::sigset_t = std::mem::zeroed();
                libc::signal(libc::SIGALRM, libc::SIG_IGN);
                libc::signal(libc::SIGXCPU, libc::SIG_IGN);
                libc::sigfillset(&mut every);
                libc::sigprocmask(libc::SIG_SETMASK, &every, std::ptr::null_mut());
                outer()
            });
        }
        let mut child = command.spawn().unwrap();
        let pid = child.id();
        let status = confined_status(pid, inherited);
        assert_eq!(field(&status, "Name"), "gitturtle-pdf");
        assert_eq!(field(&status, "NoNewPrivs"), "1");
        assert_eq!(field(&status, "Threads"), "1");
        assert_eq!(u64::from_str_radix(field(&status, "SigBlk"), 16), Ok(0));
        let ignored = u64::from_str_radix(field(&status, "SigIgn"), 16).unwrap();
        let bit = |signal: i32| 1u64 << (signal - 1);
        assert_eq!(ignored & (bit(libc::SIGALRM) | bit(libc::SIGXCPU)), 0);
        assert_ne!(ignored & bit(libc::SIGXFSZ), 0);
        let size = field(&status, "VmSize");
        let kib: u64 = size.trim_end_matches("kB").trim().parse().unwrap();
        eprintln!("the helper's VmSize at confinement: {size}");
        assert!(
            kib <= 512 * 1024,
            "VmSize {size} leaves RLIMIT_AS too little room"
        );

        let mut stack = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: reads into a local.
        assert_eq!(
            unsafe { libc::getrlimit(libc::RLIMIT_STACK, &mut stack) },
            0
        );
        let stack = (8 << 20).min(stack.rlim_cur).to_string();
        let limits = fs::read_to_string(format!("/proc/{pid}/limits")).unwrap();
        for (name, soft, hard) in [
            ("Max cpu time", "10", "11"),
            ("Max file size", "0", "0"),
            ("Max data size", "1073741824", "1073741824"),
            ("Max stack size", &stack, &stack),
            ("Max core file size", "0", "0"),
            ("Max processes", "0", "0"),
            ("Max open files", "3", "3"),
            ("Max address space", "2147483648", "2147483648"),
        ] {
            let values: Vec<_> = limits
                .lines()
                .find_map(|line| line.strip_prefix(name))
                .unwrap_or_else(|| panic!("no {name} in\n{limits}"))
                .split_whitespace()
                .collect();
            assert_eq!(values[..2], [soft, hard], "{name}");
        }
        // Not dumpable: the descriptor table belongs to root.
        let descriptors = fs::metadata(format!("/proc/{pid}/fd")).unwrap();
        assert_eq!(descriptors.uid(), 0);
        // Written while the file still belonged to the helper.
        let oom_score_adj = fs::read_to_string(format!("/proc/{pid}/oom_score_adj")).unwrap();
        assert_eq!(oom_score_adj.trim(), "1000");

        let pdf = fixtures::benign();
        let mut input = child.stdin.take().unwrap();
        input.write_all(&encode_request(0, &pdf).unwrap()).unwrap();
        input.write_all(&pdf).unwrap();
        drop(input);
        let mut reply = Vec::new();
        child
            .stdout
            .take()
            .unwrap()
            .read_to_end(&mut reply)
            .unwrap();
        assert_eq!(child.wait().unwrap().code(), Some(0));
        let page = read_reply(&reply[..]).unwrap().unwrap();
        assert_eq!((page.page, page.page_count), (0, 1));
        assert_eq!((page.image.width, page.image.height), (120, 80));
        assert_eq!(page.image.rgba[..4], [255, 0, 0, 255]);
        let last = page.image.rgba.len() - 4;
        assert_eq!(page.image.rgba[last..], [255, 255, 255, 255]);
        assert_eq!(page.text.unwrap().text.as_deref(), Some("Benign"));
    }

    fn pipe() -> (OwnedFd, OwnedFd) {
        let mut fds = [0; 2];
        // SAFETY: fills two descriptors this function then owns.
        assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) }, 0);
        // SAFETY: both were just opened.
        unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) }
    }

    /// Leaves `fd` open as `at` in the child, without close-on-exec.
    fn inherit(fd: libc::c_int, at: libc::c_int) -> io::Result<()> {
        // SAFETY: called between fork and exec on descriptors the child owns.
        let result = unsafe {
            if fd == at {
                libc::fcntl(fd, libc::F_SETFD, 0)
            } else {
                libc::dup2(fd, at)
            }
        };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[test]
    fn inherited_descriptors_close_before_input() {
        let (low_read, low_write) = pipe();
        let (high_read, high_write) = pipe();
        let (low, high) = (low_write.as_raw_fd(), high_write.as_raw_fd());
        let mut command = helper(&own_pid());
        // SAFETY: only dup2 and fcntl between fork and exec.
        unsafe {
            command.pre_exec(move || {
                inherit(high, 1000)?;
                inherit(low, 5)
            });
        }
        let mut child = command.spawn().unwrap();
        drop((low_write, high_write));
        let started = Instant::now();
        for read in [&low_read, &high_read] {
            let mut poll = libc::pollfd {
                fd: read.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: polls one descriptor this test owns.
            assert_eq!(unsafe { libc::poll(&mut poll, 1, 1000) }, 1);
            assert_ne!(poll.revents & libc::POLLHUP, 0);
        }
        assert!(started.elapsed() < Duration::from_secs(1));
        // The helper closed them itself: it still waits on stdin.
        assert!(child.try_wait().unwrap().is_none());
        confined_status(child.id(), own_filters());
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn the_helper_dies_with_its_parent() {
        let mut shell = Command::new("/bin/sh")
            .args([
                "-c",
                r#""$0" "$1" "$2" $$ <&0 & echo $!; wait"#,
                BINARY,
                HELPER_FLAG,
                &version(),
            ])
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(shell.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let helper: u32 = line.trim().parse().unwrap();
        confined_status(helper, own_filters());
        // The helper keeps waiting on this input: only its parent's death
        // can end it.
        let input = shell.stdin.take();
        shell.kill().unwrap();
        let killed = Instant::now();
        shell.wait().unwrap();
        while running(helper) {
            assert!(
                killed.elapsed() < Duration::from_millis(100),
                "the helper outlived its parent"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        drop(input);
    }

    #[test]
    fn a_wrong_parent_exits_before_input() {
        // The input stays open and unwritten.
        let mut child = helper("1").spawn().unwrap();
        assert_eq!(exit_code(&mut child), Some(exit::PARENT));
    }

    #[test]
    fn malformed_helper_arguments_exit_before_input() {
        let (version, parent) = (version(), own_pid());
        let cases = [
            ("2", parent.as_str()),
            ("01", &parent),
            ("+1", &parent),
            (" 1", &parent),
            ("", &parent),
            (&version, "0"),
            (&version, "-1"),
            (&version, "+5"),
            (&version, "05"),
            (&version, "2147483648"),
            (&version, "99999999999"),
            (&version, "1x"),
            (&version, ""),
        ];
        for (version, parent) in cases {
            let mut child = Command::new(BINARY)
                .args([HELPER_FLAG, version, parent])
                .env_clear()
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            assert_eq!(
                exit_code(&mut child),
                Some(exit::ARGUMENTS),
                "{version:?} {parent:?}"
            );
        }
    }

    #[test]
    fn every_other_argument_form_starts_the_app() {
        let (version, parent) = (version(), own_pid());
        let forms: [&[&str]; 9] = [
            &[],
            &[HELPER_FLAG],
            &["/tmp/repository"],
            &[HELPER_FLAG, &version],
            &[HELPER_FLAG, "/tmp/repository"],
            &[HELPER_FLAG, &version, &parent, "extra"],
            &["extra", HELPER_FLAG, &version, &parent],
            &["--gitturtle-pdf-helper=1", &version, &parent],
            &["/tmp/repository", &version, &parent],
        ];
        for form in forms {
            // No display, so the app path stops with its headless message
            // instead of opening a window.
            let output = Command::new(BINARY)
                .args(form)
                .env_clear()
                .env("ZED_HEADLESS", "1")
                .current_dir("/")
                .stdin(Stdio::null())
                .output()
                .unwrap();
            let message = String::from_utf8_lossy(&output.stderr);
            assert_eq!(output.status.code(), Some(1), "{form:?}: {message}");
            assert!(
                message.contains("needs a Wayland or X11 desktop session"),
                "{form:?}: {message}"
            );
        }
    }
}

/// The client against `/bin/sh -c` fakes: each outcome, cancellation, the
/// deadline and the helper limit.
mod client {
    use std::ffi::OsStr;
    use std::fs;
    use std::os::unix::process::ExitStatusExt;
    use std::panic::AssertUnwindSafe;
    use std::path::{Path, PathBuf};
    use std::process::ExitStatus;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;
    use std::time::{Duration, Instant};

    use anyhow::anyhow;
    use gitturtle_preview::pdf_helper::{ErrorKind, PROTOCOL_VERSION, PageReply};

    use super::fixtures;
    use super::pdf_helper::{
        Client, Failure, HELPER_FLAG, HELPERS, HelperCommand, Slots, classify_exit, exit,
    };

    const LONG: Duration = Duration::from_secs(30);

    /// A shell fake that first writes its pid to `pid`. `exec` keeps that pid
    /// for the program it runs.
    fn fake(pid: &Path, script: &str) -> HelperCommand {
        let script = format!(
            "PATH=/usr/bin:/bin; printf %s $$ > '{}'; {script}",
            pid.display()
        );
        HelperCommand::new("/bin/sh", ["-c", script.as_str()])
    }

    fn render_with(command: HelperCommand, deadline: Duration) -> Result<PageReply, Failure> {
        let slots = Slots::new(2);
        Client::with(command, deadline, &slots).render(&fixtures::benign(), 0, &|| Ok(()))
    }

    /// The pid a fake wrote, once it has.
    fn pid_in(path: &Path) -> libc::pid_t {
        let started = Instant::now();
        loop {
            if let Some(pid) = fs::read_to_string(path)
                .ok()
                .and_then(|pid| pid.parse().ok())
            {
                return pid;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the fake never started"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }

    /// The process is reaped: not even a zombie is left.
    fn reaped(pid: libc::pid_t) -> bool {
        // SAFETY: signal 0 only checks that the process exists.
        let result = unsafe { libc::kill(pid, 0) };
        result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    }

    fn reply_prefix(status: u8) -> Vec<u8> {
        let mut frame = b"GTPr".to_vec();
        frame.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
        frame.push(status);
        frame
    }

    /// An error reply of `code` with `detail`.
    fn error_frame(code: u8, detail: u32) -> Vec<u8> {
        let mut frame = reply_prefix(1);
        frame.push(code);
        frame.extend_from_slice(&detail.to_le_bytes());
        frame
    }

    /// A 1 × 1 page reply for `page` of two pages, with `text`.
    fn page_frame(page: u32, text: &str) -> Vec<u8> {
        let mut frame = reply_prefix(0);
        for value in [page, 2, 1, 1, 1, 1] {
            frame.extend_from_slice(&value.to_le_bytes());
        }
        frame.extend_from_slice(&[1, 0, 0]);
        for value in [text.len() as u32, 1, 2, 3] {
            frame.extend_from_slice(&value.to_le_bytes());
        }
        frame.extend_from_slice(&[9, 8, 7, 255]);
        frame.extend_from_slice(text.as_bytes());
        frame
    }

    struct Fakes {
        directory: tempfile::TempDir,
    }

    impl Fakes {
        fn new() -> Self {
            Self {
                directory: tempfile::tempdir().unwrap(),
            }
        }

        fn path(&self, name: &str) -> PathBuf {
            self.directory.path().join(name)
        }

        /// A file holding `bytes`, for a fake to `cat`.
        fn file(&self, name: &str, bytes: &[u8]) -> String {
            let path = self.path(name);
            fs::write(&path, bytes).unwrap();
            format!("'{}'", path.display())
        }
    }

    #[test]
    fn each_helper_failure_has_its_own_outcome_and_leaves_no_process() {
        let fakes = Fakes::new();
        let locked = fakes.file("locked", &error_frame(6, 0));
        let mut trailing = error_frame(6, 0);
        trailing.extend_from_slice(b"SECRET");
        let trailing = fakes.file("trailing", &trailing);
        let unknown = fakes.file("unknown", &error_frame(99, 0));
        let wrong_page = fakes.file("wrong-page", &page_frame(1, "SECRET"));
        let far_count = fakes.file("far-count", &error_frame(9, 5));
        let renderer = fakes.file("renderer", &error_frame(13, 0));
        type Expected = fn(&Failure) -> bool;
        let cases: Vec<(&str, String, Expected)> = vec![
            ("garbage", "printf SECRET-garbage".into(), |f| {
                matches!(f, Failure::Unreadable)
            }),
            (
                "oversized",
                "head -c 5000000 /dev/zero; exec sleep 30".into(),
                |f| matches!(f, Failure::Unreadable),
            ),
            ("extra bytes", format!("exec cat {trailing}"), |f| {
                matches!(f, Failure::Unreadable)
            }),
            ("unknown kind", format!("exec cat {unknown}"), |f| {
                matches!(f, Failure::Unreadable)
            }),
            ("wrong page", format!("exec cat {wrong_page}"), |f| {
                matches!(f, Failure::Unreadable)
            }),
            (
                "count past the page",
                format!("exec cat {far_count}"),
                |f| matches!(f, Failure::Unreadable),
            ),
            ("no reply", "exit 0".into(), |f| {
                matches!(f, Failure::Unreadable)
            }),
            ("early exit", "echo SECRET >&2; exit 3".into(), |f| {
                matches!(f, Failure::Stopped { signal: None })
            }),
            (
                "valid reply, bad exit",
                format!("cat {locked}; exit 1"),
                |f| matches!(f, Failure::Stopped { signal: None }),
            ),
            ("renderer failure", format!("exec cat {renderer}"), |f| {
                matches!(
                    f,
                    Failure::Document {
                        kind: ErrorKind::RendererFailed,
                        ..
                    }
                )
            }),
            // Only deaths that dump no core: signals that would are covered
            // by `every_ending_is_classified` with synthesized statuses.
            (
                "SIGKILL not sent by the client",
                "kill -KILL $$".into(),
                |f| {
                    matches!(
                        f,
                        Failure::Stopped {
                            signal: Some(libc::SIGKILL)
                        }
                    )
                },
            ),
            ("alarm", "kill -ALRM $$".into(), |f| {
                matches!(f, Failure::TimedOut)
            }),
            (
                "confinement unavailable",
                format!("exit {}", exit::CONFINEMENT),
                |f| matches!(f, Failure::CouldNotStart),
            ),
            ("setup failure", format!("exit {}", exit::SETUP), |f| {
                matches!(f, Failure::CouldNotStart)
            }),
            ("wrong parent", format!("exit {}", exit::PARENT), |f| {
                matches!(f, Failure::CouldNotStart)
            }),
            ("reply not written", format!("exit {}", exit::REPLY), |f| {
                matches!(f, Failure::Stopped { signal: None })
            }),
            ("document error", format!("exec cat {locked}"), |f| {
                matches!(
                    f,
                    Failure::Document {
                        kind: ErrorKind::Locked,
                        page: 0
                    }
                )
            }),
        ];
        // Every case runs, so one report names every wrong outcome.
        let mut wrong = Vec::new();
        for (index, (case, script, expected)) in cases.iter().enumerate() {
            let pid = fakes.path(&format!("pid-{index}"));
            let started = Instant::now();
            let result = render_with(fake(&pid, script), LONG);
            let elapsed = started.elapsed();
            let Err(failure) = result else {
                wrong.push(format!("{case}: a page"));
                continue;
            };
            // Helper bytes never reach the wording.
            let message = failure.to_string();
            if !expected(&failure) || message.contains("SECRET") {
                wrong.push(format!("{case}: {failure:?}, {message}"));
            }
            if elapsed >= Duration::from_secs(5) {
                wrong.push(format!("{case} waited {elapsed:?}"));
            }
            if !reaped(pid_in(&pid)) {
                wrong.push(format!("{case} left a process"));
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
        let missing = HelperCommand::new("/nonexistent/gitturtle-helper", [""; 0]);
        let failure = render_with(missing, LONG).unwrap_err();
        assert!(matches!(failure, Failure::CouldNotStart), "{failure:?}");
        assert_eq!(failure.to_string(), "The PDF renderer could not start.");
    }

    #[test]
    fn a_valid_reply_returns_the_page_with_the_helper_text_as_page_text() {
        let fakes = Fakes::new();
        let page = fakes.file("page", &page_frame(0, "SECRET"));
        let reply =
            render_with(fake(&fakes.path("pid"), &format!("exec cat {page}")), LONG).unwrap();
        assert_eq!((reply.page, reply.page_count), (0, 2));
        assert_eq!(reply.image.rgba, [9, 8, 7, 255]);
        assert_eq!(reply.text.unwrap().text.as_deref(), Some("SECRET"));
        assert!(reaped(pid_in(&fakes.path("pid"))));
        // A count no larger than the requested page is a document error.
        let out_of_range = fakes.file("out-of-range", &error_frame(9, 1));
        let slots = Slots::new(2);
        let failure = Client::with(
            fake(&fakes.path("pid-2"), &format!("exec cat {out_of_range}")),
            LONG,
            &slots,
        )
        .render(&fixtures::benign(), 3, &|| Ok(()))
        .unwrap_err();
        assert!(
            matches!(
                failure,
                Failure::Document {
                    kind: ErrorKind::PageOutOfRange { page_count: 1 },
                    page: 3
                }
            ),
            "{failure:?}"
        );
        assert_eq!(
            failure.to_string(),
            "PDF page 4 is outside this document's 1 page"
        );
    }

    #[test]
    fn a_panicking_check_still_kills_and_reaps_the_helper_and_frees_its_slot() {
        let fakes = Fakes::new();
        let locked = fakes.file("locked", &error_frame(6, 0));
        let slots = Slots::new(1);
        let pid_path = fakes.path("pid");
        let check = || {
            if pid_path.exists() {
                panic!("the caller's check panicked");
            }
            Ok(())
        };
        let request = AssertUnwindSafe(|| {
            Client::with(fake(&pid_path, "exec sleep 30"), LONG, &slots).render(
                &fixtures::benign(),
                0,
                &check,
            )
        });
        assert!(std::panic::catch_unwind(request).is_err());
        assert!(reaped(pid_in(&pid_path)));
        // The only slot is free again.
        let started = Instant::now();
        let failure = Client::with(
            fake(&fakes.path("pid-2"), &format!("exec cat {locked}")),
            LONG,
            &slots,
        )
        .render(
            &fixtures::benign(),
            0,
            &|| match started.elapsed() > Duration::from_secs(5) {
                true => Err(anyhow!("the slot stayed taken")),
                false => Ok(()),
            },
        )
        .unwrap_err();
        assert!(
            matches!(
                failure,
                Failure::Document {
                    kind: ErrorKind::Locked,
                    ..
                }
            ),
            "{failure:?}"
        );
    }

    fn thread_cpu_time() -> Duration {
        let mut time = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: writes into a local.
        assert_eq!(
            unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut time) },
            0
        );
        Duration::new(time.tv_sec as u64, time.tv_nsec as u32)
    }

    #[test]
    fn a_helper_that_closes_its_output_early_is_waited_for_without_spinning() {
        let fakes = Fakes::new();
        let locked = fakes.file("locked", &error_frame(6, 0));
        let script = format!("cat {locked}; exec >&-; exec sleep 2");
        let (started, cpu) = (Instant::now(), thread_cpu_time());
        let failure = render_with(fake(&fakes.path("pid"), &script), LONG).unwrap_err();
        let (elapsed, spent) = (started.elapsed(), thread_cpu_time() - cpu);
        eprintln!("waited {elapsed:?} with {spent:?} of CPU time");
        assert!(
            matches!(
                failure,
                Failure::Document {
                    kind: ErrorKind::Locked,
                    ..
                }
            ),
            "{failure:?}"
        );
        // The reply counts once the helper exits, not when its output closes.
        assert!(elapsed >= Duration::from_secs(2), "{elapsed:?}");
        // The wait sleeps until the exit instead of waking every millisecond.
        assert!(spent < Duration::from_millis(5), "{spent:?}");
        assert!(reaped(pid_in(&fakes.path("pid"))));
    }

    /// Wait statuses built here: a fake helper killed by SIGSYS, SIGABRT or
    /// SIGXCPU would leave a core file from the shell.
    #[test]
    fn every_ending_is_classified() {
        let signaled = |signal: i32| ExitStatus::from_raw(signal);
        let exited = |code: i32| ExitStatus::from_raw(code << 8);
        let page = page_frame(0, "text");
        for signal in [
            libc::SIGSYS,
            libc::SIGABRT,
            libc::SIGSEGV,
            libc::SIGBUS,
            libc::SIGKILL,
            libc::SIGTERM,
        ] {
            let failure = classify_exit(signaled(signal), &page, 0).unwrap_err();
            assert!(
                matches!(failure, Failure::Stopped { signal: Some(by) } if by == signal),
                "{signal}: {failure:?}"
            );
        }
        for signal in [libc::SIGXCPU, libc::SIGALRM] {
            let result = classify_exit(signaled(signal), &page, 0);
            assert!(matches!(result, Err(Failure::TimedOut)), "{signal}");
        }
        for code in [
            exit::ARGUMENTS,
            exit::PARENT,
            exit::SETUP,
            exit::CONFINEMENT,
        ] {
            let result = classify_exit(exited(code), &page, 0);
            assert!(matches!(result, Err(Failure::CouldNotStart)), "{code}");
        }
        for code in [exit::REPLY, 1, 3, 255] {
            let result = classify_exit(exited(code), &page, 0);
            assert!(
                matches!(result, Err(Failure::Stopped { signal: None })),
                "{code}"
            );
        }
        let reply = classify_exit(exited(0), &page, 0).unwrap();
        assert_eq!(reply.text.unwrap().text.as_deref(), Some("text"));
        let empty = classify_exit(exited(0), b"", 0);
        assert!(matches!(empty, Err(Failure::Unreadable)));
    }

    #[test]
    fn the_app_starts_this_executable_with_the_exact_helper_arguments() {
        let expected = [
            "/proc/self/exe".to_string(),
            HELPER_FLAG.to_string(),
            PROTOCOL_VERSION.to_string(),
            std::process::id().to_string(),
        ];
        let expected = expected.each_ref().map(OsStr::new);
        assert_eq!(HelperCommand::current().argv(), expected);
        let client = Client::new();
        let (command, deadline, slots) = client.parts();
        assert_eq!(command.argv(), expected);
        assert_eq!(deadline, Duration::from_secs(10));
        assert!(std::ptr::eq(slots, &HELPERS));
    }

    #[test]
    fn refused_input_starts_no_helper() {
        let fakes = Fakes::new();
        let slots = Slots::new(2);
        let client = Client::with(fake(&fakes.path("pid"), "exit 0"), LONG, &slots);
        let failure = client.render(b"not a pdf", 0, &|| Ok(())).unwrap_err();
        assert!(matches!(failure, Failure::Refused(_)), "{failure:?}");
        assert_eq!(failure.to_string(), "Unrecognized PDF header");
        // Time for a wrongly started fake to write its pid.
        thread::sleep(Duration::from_millis(200));
        assert!(!fakes.path("pid").exists());
    }

    #[test]
    fn the_deadline_kills_and_reaps_the_helper() {
        let fakes = Fakes::new();
        let slots = Slots::new(2);
        let deadline = Duration::from_millis(300);
        let pid_path = fakes.path("pid");
        thread::scope(|scope| {
            let request = scope.spawn(|| {
                let called = Instant::now();
                let result = Client::with(fake(&pid_path, "exec sleep 30"), deadline, &slots)
                    .render(&fixtures::benign(), 0, &|| Ok(()));
                (result, called, Instant::now())
            });
            let pid = pid_in(&pid_path);
            // The deadline counts from the spawn, which precedes the pid file,
            // so it passes no later than this.
            let deadline_by = Instant::now() + deadline;
            let (result, called, reaped_at) = request.join().unwrap();
            let failure = result.unwrap_err();
            assert!(matches!(failure, Failure::TimedOut), "{failure:?}");
            assert_eq!(
                failure.to_string(),
                "The PDF page took longer than 10 seconds and was stopped."
            );
            assert!(reaped_at - called >= deadline);
            // From the deadline to the reap: a 25 ms tick, the kill and the
            // wait, with room for a busy host.
            let late = reaped_at.saturating_duration_since(deadline_by);
            assert!(late < Duration::from_millis(100), "{late:?}");
            assert!(reaped(pid));
        });
    }

    #[test]
    fn cancellation_leaves_no_helper_within_100_ms() {
        let fakes = Fakes::new();
        let slots = Slots::new(2);
        let cancelled = AtomicBool::new(false);
        let check = || match cancelled.load(Ordering::SeqCst) {
            true => Err(anyhow!("A newer page replaced this one")),
            false => Ok(()),
        };
        let pid_path = fakes.path("pid");
        thread::scope(|scope| {
            let request =
                scope.spawn(|| {
                    let result = Client::with(fake(&pid_path, "exec sleep 30"), LONG, &slots)
                        .render(&fixtures::benign(), 0, &check);
                    (result, Instant::now())
                });
            let pid = pid_in(&pid_path);
            let cancelled_at = Instant::now();
            cancelled.store(true, Ordering::SeqCst);
            let (result, returned) = request.join().unwrap();
            let failure = result.unwrap_err();
            assert_eq!(failure.to_string(), "A newer page replaced this one");
            assert!(matches!(failure, Failure::Cancelled(_)));
            assert!(reaped(pid));
            assert!(returned - cancelled_at < Duration::from_millis(100));
        });
    }

    #[test]
    fn at_most_two_helpers_run_and_a_waiting_request_still_cancels() {
        let fakes = Fakes::new();
        let locked = fakes.file("locked", &error_frame(6, 0));
        let slots = Slots::new(2);
        let release = AtomicBool::new(false);
        let running_check = || match release.load(Ordering::SeqCst) {
            true => Err(anyhow!("released")),
            false => Ok(()),
        };
        let cancel_waiting = AtomicBool::new(false);
        let waiting_check = || match cancel_waiting.load(Ordering::SeqCst) {
            true => Err(anyhow!("cancelled while waiting")),
            false => Ok(()),
        };
        let pids = [0, 1, 2, 3].map(|index| fakes.path(&format!("pid-{index}")));
        let pdf = fixtures::benign();
        thread::scope(|scope| {
            let running = [0, 1].map(|index| {
                let (pid, slots, pdf, check) = (&pids[index], &slots, &pdf, &running_check);
                scope.spawn(move || {
                    Client::with(fake(pid, "exec sleep 30"), LONG, slots).render(pdf, 0, check)
                })
            });
            let started = [pid_in(&pids[0]), pid_in(&pids[1])];
            let waiting = scope.spawn(|| {
                let result = Client::with(fake(&pids[2], "exit 0"), LONG, &slots).render(
                    &pdf,
                    0,
                    &waiting_check,
                );
                (result, Instant::now())
            });
            // Give the waiting request time to start a helper it must not.
            thread::sleep(Duration::from_millis(150));
            assert!(!pids[2].exists(), "a third helper started");
            let cancelled_at = Instant::now();
            cancel_waiting.store(true, Ordering::SeqCst);
            let (result, returned) = waiting.join().unwrap();
            assert_eq!(result.unwrap_err().to_string(), "cancelled while waiting");
            assert!(returned - cancelled_at < Duration::from_millis(100));
            assert!(!pids[2].exists());

            // A later request starts once a running one ends.
            let later =
                scope.spawn(|| {
                    Client::with(fake(&pids[3], &format!("exec cat {locked}")), LONG, &slots)
                        .render(&pdf, 0, &|| Ok(()))
                });
            thread::sleep(Duration::from_millis(150));
            assert!(!pids[3].exists(), "a third helper started");
            release.store(true, Ordering::SeqCst);
            for request in running {
                assert!(matches!(
                    request.join().unwrap(),
                    Err(Failure::Cancelled(_))
                ));
            }
            assert!(started.iter().all(|&pid| reaped(pid)));
            let failure = later.join().unwrap().unwrap_err();
            assert!(
                matches!(
                    failure,
                    Failure::Document {
                        kind: ErrorKind::Locked,
                        ..
                    }
                ),
                "{failure:?}"
            );
        });
    }
}

/// Hostile hayro inputs through the client and the real, confined binary,
/// each rebuilt from its issue. Section 6 of the architecture review sets
/// how each ends; none may meet the filter.
mod hostile {
    use std::time::{Duration, Instant};

    use gitturtle_preview::pdf_helper::{ErrorKind, PROTOCOL_VERSION, PageReply};

    use super::fixtures::{assemble, page, stream};
    use super::pdf_helper::{Client, Failure, HELPER_FLAG, HelperCommand, Slots};

    const BINARY: &str = env!("CARGO_BIN_EXE_gitturtle");
    /// Shorter than the app's 10 s so the endless inputs end sooner; the
    /// helper's CPU limit and alarm stay later than either.
    const DEADLINE: Duration = Duration::from_secs(3);

    /// https://github.com/LaurenzV/hayro/issues/1549: a 100000 × 100000
    /// single-component codestream with no decomposition levels.
    const JPX_1549: &str = "ff4fff5100290000000186a0000186a00000000000000000000186a0000186a000000000000000000001070101ff52000c00000001000004040001ff5c00044040ff90000a0000000000000001ff9300000000000000000000000000000000ffd9";
    /// https://github.com/LaurenzV/hayro/issues/1550 (the 2026-11-01
    /// reproducer): a halftone region of 64786 × 24332 cells.
    const JBIG2_1550: &str = "00000000300001000000130000009b0000006500001ec200001ec2010000000000011001010000001c00090900000003e9fe4aea9b3fcbf636e6d528b7efc58f9641bfffac00000002172001010000005f0000009b00000065000000000000000000000000fd1200005f0c00003f0000000000090000001e44d543ec5900b651bf1d5af117e9cd03ca8621010a5feb0b93bdf2457f5a40ef594395eefb557f0d84764b15892809a54eccf9e86851ffac0000000331000100000000";
    /// https://github.com/LaurenzV/hayro/issues/1598 (the 2026-11-01
    /// reproducer): a symbol dictionary that takes 38.9 s in release.
    const JBIG2_1598: &str = "0000000030000100000013000000a30000006d00001ec200001ec2010000000000010001010000005d000003fffdff02fefefe00000004000000fe2ad8de99ef13e94932eee8357fc80ae15399fd6506245d84fc71418296d3754909f9d98b18667bbfa1ef79d6494c4811cea9ff520ef87046190be5fe1ae8bf15dfa2f4c6c025f85035ff5500000002072001010000004f000000a30000006d00000000000000fd001b180000005a98f4702d3fedd5814de8a0a8d33ea0bb7c21e2a3553e56d595c58c794c355eb6b138beb8a8210221fe35ad84411d042bc434c9132927ffac0000ef0331000100000000";
    /// https://github.com/LaurenzV/hayro/issues/1700: `export_symbols` never
    /// advances once the export run lengths are 0.
    const JBIG2_1700: &str = "0000000030000100000013000000a30000006d00001ec200001ec2010000000000010001010000005d000003fffdff02fefefe00000004000000042a8ade999613e94932eee83551c80ae15399fd6506245d849c71418296d3754909f9d98b18667bbfcf8f79d6494c4811cea9ff520ef87046190be51a1ae8bf15dfa2f4c6c025cf5035ffac00000002072001010000004f000000a30000006d0000000000000000000c180000005a98f4702d3fedd5814de8a0a8d33ea0bb7c21e2a3553e56d595c58c794c355eb6b138beb8a8210221fe35ad84411d042bc434c9132927ffac0000000331000100000000";

    fn render(pdf: &[u8]) -> (Result<PageReply, Failure>, Duration) {
        let command = HelperCommand::new(
            BINARY,
            [
                HELPER_FLAG.to_string(),
                PROTOCOL_VERSION.to_string(),
                std::process::id().to_string(),
            ],
        );
        // One at a time: a helper may hold its whole 1 GiB.
        let slots = Slots::new(1);
        let started = Instant::now();
        let result = Client::with(command, DEADLINE, &slots).render(pdf, 0, &|| Ok(()));
        (result, started.elapsed())
    }

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap())
            .collect()
    }

    /// A page that draws `data` as `/Im0` over the page with the image's
    /// other `dictionary` entries.
    fn image(dictionary: &str, data: &[u8]) -> Vec<u8> {
        page(
            b"q 120 0 0 80 0 0 cm /Im0 Do Q",
            Some(stream(
                &format!("/Type /XObject /Subtype /Image {dictionary}"),
                data,
            )),
        )
    }

    fn jbig2(width: u32, height: u32, data: &str) -> Vec<u8> {
        image(
            &format!(
                "/Width {width} /Height {height} /ColorSpace /DeviceGray /BitsPerComponent 1 /Filter /JBIG2Decode"
            ),
            &hex(data),
        )
    }

    /// A page with `resources` whose only content stream is `contents`,
    /// encoded with `filter`.
    fn custom_page(resources: &str, filter: &str, contents: &[u8]) -> Vec<u8> {
        assemble(&[
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 80] /Resources << {resources} >> /Contents 4 0 R >>"
            )
            .into_bytes(),
            stream(filter, contents),
        ])
    }

    /// Bits written least significant first, as deflate packs them.
    #[derive(Default)]
    struct Bits {
        bytes: Vec<u8>,
        pending: u64,
        count: u32,
    }

    impl Bits {
        fn put(&mut self, value: u32, bits: u32) {
            self.pending |= u64::from(value) << self.count;
            self.count += bits;
            while self.count >= 8 {
                self.bytes.push(self.pending as u8);
                self.pending >>= 8;
                self.count -= 8;
            }
        }

        /// A Huffman code goes most significant bit first.
        fn code(&mut self, code: u32, bits: u32) {
            self.put(code.reverse_bits() >> (32 - bits), bits);
        }
    }

    /// A zlib stream of `length` zero bytes: one fixed-Huffman block with a
    /// literal zero, then copies of 258 bytes at distance 1, 13 bits each.
    fn zeros_zlib(length: usize) -> Vec<u8> {
        let mut bits = Bits {
            bytes: vec![0x78, 0x01],
            ..Bits::default()
        };
        bits.put(1, 1); // the final block
        bits.put(1, 2); // fixed Huffman codes
        bits.code(0x30, 8); // literal 0
        for _ in 0..(length - 1) / 258 {
            bits.code(0xc5, 8); // length 258
            bits.code(0, 5); // distance 1
        }
        for _ in 0..(length - 1) % 258 {
            bits.code(0x30, 8);
        }
        bits.code(0, 7); // end of block
        bits.put(0, 7); // pad to a byte
        let mut bytes = bits.bytes;
        // Adler-32 of zeros: the sum stays 1, so the second sum is the length.
        let adler = ((length % 65_521) as u32) << 16 | 1;
        bytes.extend_from_slice(&adler.to_be_bytes());
        bytes
    }

    type Expected = fn(&Result<PageReply, Failure>) -> bool;

    fn signaled(result: &Result<PageReply, Failure>, signals: &[i32]) -> bool {
        matches!(result, Err(Failure::Stopped { signal: Some(signal) }) if signals.contains(signal))
    }

    /// Every hostile input ends within the client's deadline and a second,
    /// in the way its case names, and none meets the seccomp filter
    /// (SIGSYS). The flate bomb ends either way a bound stops it: by SIGABRT
    /// or an error kind where its inflated content meets the 1 GiB data
    /// limit, or by the client's deadline kill where inflating it takes
    /// longer than the deadline. It once had to abort, which it does in
    /// about 0.9 s in a debug build on the AArch64 host, but a loaded hosted
    /// x86-64 runner was still inflating it at the 3 s deadline (run
    /// 41513461121), as this host is every time at a 10% CPU quota. A kill
    /// is the bound the endless JBIG2 inputs meet and the app's timeout
    /// sentence; what the bomb must never do is finish, meet the filter or
    /// outlast the deadline. #1588's oversized allocation stays the memory
    /// limits' evidence, an abort in milliseconds at any speed, and
    /// `/proc/<pid>/limits` pins the data limit's value.
    #[test]
    fn hostile_inputs_end_within_the_deadline_and_never_meet_the_filter() {
        // https://github.com/LaurenzV/hayro/issues/1610: finding the end of
        // an inline image parses the rest of the stream, one frame per image.
        let mut recursion = b"q BI /W 1 /H 1 /BPC 8 /CS /G ID x EI Q %EI\n".repeat(12_000);
        recursion.extend_from_slice(b"(x) Tj\n");
        // https://github.com/LaurenzV/hayro/issues/1546 (fixed after 0.8.0):
        // a Separation tint function that returns one component for DeviceRGB
        // panics while the fill colour is converted. It stands in for #1649,
        // whose JPEG 2000 panics need palette resolution, which hayro's
        // JPXDecode filter never asks for.
        let separation = "/ColorSpace << /Cs1 [/Separation /Spot /DeviceRGB << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >>] >>";
        let aborted: Expected = |result| {
            signaled(result, &[libc::SIGABRT]) || matches!(result, Err(Failure::Document { .. }))
        };
        let timed_out: Expected = |result| matches!(result, Err(Failure::TimedOut));
        let cases: [(&str, Vec<u8>, Expected); 10] = [
            (
                "benign",
                super::fixtures::benign(),
                |result| matches!(result, Ok(page) if page.image.rgba[..4] == [255, 0, 0, 255]),
            ),
            (
                "#1546 panic",
                custom_page(separation, "", b"/Cs1 cs 0.5 sc 0 0 50 50 re f"),
                |result| {
                    matches!(
                        result,
                        Err(Failure::Document {
                            kind: ErrorKind::RendererFailed,
                            ..
                        })
                    )
                },
            ),
            (
                "#1549",
                image(
                    "/Width 100000 /Height 100000 /Filter /JPXDecode",
                    &hex(JPX_1549),
                ),
                aborted,
            ),
            // https://github.com/LaurenzV/hayro/issues/1588: bit-packed samples
            // become a byte each, however short the data.
            (
                "#1588",
                image(
                    "/Width 60000 /Height 60000 /ColorSpace /DeviceGray /BitsPerComponent 1",
                    b"U",
                ),
                |result| signaled(result, &[libc::SIGABRT]),
            ),
            ("#1610", custom_page("", "", &recursion), |result| {
                signaled(result, &[libc::SIGABRT, libc::SIGSEGV])
            }),
            ("#1550", jbig2(155, 101, JBIG2_1550), aborted),
            ("#1598", jbig2(163, 109, JBIG2_1598), timed_out),
            ("#1700", jbig2(163, 109, JBIG2_1700), timed_out),
            // 1.25 GiB of content from 8.5 MB of FlateDecode data: the data
            // limit aborts it once inflated, unless the deadline kills it
            // first (see the test's comment).
            (
                "flate bomb",
                custom_page("", "/Filter /FlateDecode", &zeros_zlib(5 << 28)),
                |result| {
                    signaled(result, &[libc::SIGABRT])
                        || matches!(result, Err(Failure::Document { .. } | Failure::TimedOut))
                },
            ),
            // The bomb's encoder decodes: a black 10 × 10 image.
            (
                "small flate image",
                image(
                    "/Width 10 /Height 10 /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode",
                    &zeros_zlib(100),
                ),
                |result| matches!(result, Ok(page) if page.image.rgba[..4] == [0, 0, 0, 255]),
            ),
        ];
        for (case, pdf, expected) in cases {
            let (result, elapsed) = render(&pdf);
            let outcome = result.as_ref().map(|page| page.page_count);
            eprintln!("{case}: {outcome:?} after {elapsed:?}");
            assert!(!signaled(&result, &[libc::SIGSYS]), "{case} met the filter");
            assert!(expected(&result), "{case}: {outcome:?}");
            assert!(
                elapsed < DEADLINE + Duration::from_secs(1),
                "{case} took {elapsed:?}"
            );
        }
    }
}
