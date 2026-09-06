#![cfg(unix)]

use std::{
    env,
    fs::File,
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::process::CommandExt,
    },
    path::Path,
    process::{Command, ExitStatus, Stdio},
    thread,
    time::Duration,
};

use bit_mail::repository::{GitIgnorePolicy, NewAccount, Repository};

fn pty_pair(columns: u16) -> io::Result<(File, File)> {
    let mut master = -1;
    let mut slave = -1;
    let window = libc::winsize {
        ws_row: 24,
        ws_col: columns,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let result = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &window,
        )
    };
    if result == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok((unsafe { File::from_raw_fd(master) }, unsafe {
        File::from_raw_fd(slave)
    }))
}

fn read_pty(mut master: File) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let mut pollfd = libc::pollfd {
            fd: master.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut pollfd, 1, 1000) };
        if ready == 0 {
            return Ok(output);
        }
        if ready == -1 {
            return Err(io::Error::last_os_error());
        }
        match master.read(&mut buffer) {
            Ok(0) => return Ok(output),
            Ok(read) => output.extend_from_slice(&buffer[..read]),
            Err(error) if error.raw_os_error() == Some(libc::EIO) => return Ok(output),
            Err(error) => return Err(error),
        }
    }
}

fn read_pipe(mut pipe: impl Read) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    pipe.read_to_end(&mut output)?;
    Ok(output)
}

#[allow(clippy::too_many_arguments)]
fn run_pty(
    program: &Path,
    directory: &Path,
    arguments: &[&str],
    columns: u16,
    term: &str,
    no_color: bool,
    stdout_terminal: bool,
    stderr_terminal: bool,
    extra_env: Option<(&str, &str)>,
) -> io::Result<(ExitStatus, Vec<u8>, Vec<u8>)> {
    let (stdout_master, stdout_slave) = pty_pair(columns)?;
    let (mut stderr_master, stderr_slave) = if stderr_terminal {
        let (master, slave) = pty_pair(columns)?;
        (Some(master), Some(slave))
    } else {
        (None, None)
    };
    let mut command = Command::new(program);
    command
        .current_dir(directory)
        .args(arguments)
        .env("TERM", term)
        .env_remove("COLUMNS")
        .env_remove("NO_COLOR")
        .stdin(Stdio::from(stdout_slave.try_clone()?));
    if stdout_terminal {
        command.stdout(Stdio::from(stdout_slave.try_clone()?));
    } else {
        command.stdout(Stdio::piped());
    }
    if let Some(stderr_slave) = stderr_slave {
        command.stderr(Stdio::from(stderr_slave));
    } else {
        command.stderr(Stdio::piped());
    }
    if no_color {
        command.env("NO_COLOR", "1");
    }
    if let Some((key, value)) = extra_env {
        command.env(key, value);
    }
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            if libc::ioctl(0, libc::TIOCSCTTY as libc::Ioctl, 0) == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn()?;
    drop(stdout_slave);
    let stdout_reader = if stdout_terminal {
        thread::spawn(move || read_pty(stdout_master))
    } else {
        let stdout = child.stdout.take().expect("stdout is piped");
        thread::spawn(move || read_pipe(stdout))
    };
    let stderr_reader = if stderr_terminal {
        thread::spawn(move || read_pty(stderr_master.take().expect("stderr PTY exists")))
    } else {
        let stderr = child.stderr.take().expect("stderr is piped");
        thread::spawn(move || read_pipe(stderr))
    };
    let status = child.wait()?;
    let stdout = stdout_reader
        .join()
        .map_err(|_| io::Error::other("stdout PTY reader panicked"))??;
    let stderr = stderr_reader
        .join()
        .map_err(|_| io::Error::other("stderr PTY reader panicked"))??;
    Ok((status, stdout, stderr))
}

fn run_bit_mail(
    directory: &Path,
    arguments: &[&str],
    columns: u16,
    term: &str,
    no_color: bool,
    stdout_terminal: bool,
    stderr_terminal: bool,
) -> io::Result<(ExitStatus, Vec<u8>, Vec<u8>)> {
    run_pty(
        Path::new(env!("CARGO_BIN_EXE_bit-mail")),
        directory,
        arguments,
        columns,
        term,
        no_color,
        stdout_terminal,
        stderr_terminal,
        None,
    )
}

fn strip_ansi(input: &str) -> String {
    let mut output = String::new();
    let mut escape = false;
    for character in input.chars() {
        if escape {
            if character.is_ascii_alphabetic() {
                escape = false;
            }
        } else if character == '\x1b' {
            escape = true;
        } else {
            output.push(character);
        }
    }
    output
}

#[test]
fn terminal_status_uses_real_width_and_independent_color_rules() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::initialize(directory.path(), GitIgnorePolicy::Never).unwrap();
    repository
        .create_account(NewAccount {
            alias: "personal",
            provider: "gmail",
            provider_identity: Some("person@example.com"),
            credential_profile: None,
        })
        .unwrap();

    let (status, automatic_help, stderr) = run_bit_mail(
        directory.path(),
        &["--help"],
        40,
        "xterm",
        false,
        true,
        true,
    )
    .unwrap();
    assert!(status.success());
    assert!(stderr.is_empty());
    let (status, explicit_help, stderr) =
        run_bit_mail(directory.path(), &["help"], 40, "xterm", false, true, true).unwrap();
    assert!(status.success());
    assert!(stderr.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&explicit_help).trim_end(),
        String::from_utf8_lossy(&automatic_help).trim_end()
    );
    assert!(
        automatic_help
            .windows(5)
            .any(|window| window == b"\x1b[36m")
    );

    let (status, stdout, stderr) = run_bit_mail(
        directory.path(),
        &["status", "--help"],
        40,
        "xterm",
        false,
        true,
        true,
    )
    .unwrap();
    assert!(status.success());
    assert!(stdout.windows(5).any(|window| window == b"\x1b[36m"));
    assert!(stderr.is_empty());
    let (status, stdout, stderr) = run_bit_mail(
        directory.path(),
        &["status", "--help"],
        40,
        "dumb",
        false,
        true,
        true,
    )
    .unwrap();
    assert!(status.success());
    assert!(!stdout.contains(&0x1b));
    assert!(!stderr.contains(&0x1b));

    let (status, stdout, stderr) = run_bit_mail(
        directory.path(),
        &["status", "--help"],
        40,
        "xterm",
        true,
        true,
        true,
    )
    .unwrap();
    assert!(status.success());
    assert!(!stdout.contains(&0x1b));
    assert!(!stderr.contains(&0x1b));

    let (status, stdout, stderr) = run_bit_mail(
        directory.path(),
        &["status", "--all-accounts"],
        40,
        "xterm",
        false,
        true,
        true,
    )
    .unwrap();
    assert!(status.success());
    assert!(stderr.is_empty());
    let rendered = String::from_utf8(stdout).unwrap();
    assert!(rendered.contains("\x1b[36m"));
    for line in strip_ansi(&rendered).replace('\r', "").lines() {
        assert!(line.chars().count() <= 40, "line is too wide: {line:?}");
    }

    let (status, stdout, _) = run_bit_mail(
        directory.path(),
        &["doctor"],
        40,
        "xterm",
        false,
        true,
        true,
    )
    .unwrap();
    assert!(!status.success());
    let doctor = String::from_utf8(stdout).unwrap();
    assert!(doctor.contains("╭") && doctor.contains("Doctor"));
    for line in strip_ansi(&doctor).replace('\r', "").lines() {
        assert!(
            line.chars().count() <= 40,
            "doctor line is too wide: {line:?}"
        );
    }

    let (status, stdout, stderr) = run_bit_mail(
        directory.path(),
        &["status", "--all-accounts"],
        40,
        "xterm",
        true,
        true,
        true,
    )
    .unwrap();
    assert!(status.success());
    assert!(!stdout.contains(&0x1b));
    assert!(!stderr.contains(&0x1b));

    let (status, stdout, stderr) = run_bit_mail(
        directory.path(),
        &["--account", "missing", "status"],
        40,
        "xterm",
        false,
        true,
        true,
    )
    .unwrap();
    assert!(!status.success());
    assert!(stdout.is_empty());
    assert!(stderr.contains(&0x1b));
    assert!(
        stderr.windows(4).any(|window| window == b"[31m"),
        "stderr: {:?}",
        String::from_utf8_lossy(&stderr)
    );

    let (status, stdout, stderr) = run_bit_mail(
        directory.path(),
        &["--account", "missing", "status"],
        40,
        "xterm",
        true,
        true,
        true,
    )
    .unwrap();
    assert!(!status.success());
    assert!(stdout.is_empty());
    assert!(!stderr.contains(&0x1b));

    let (status, stdout, stderr) = run_bit_mail(
        directory.path(),
        &["status", "--all-accounts"],
        40,
        "dumb",
        false,
        true,
        true,
    )
    .unwrap();
    assert!(status.success());
    assert!(!stdout.contains(&0x1b));
    assert!(!stderr.contains(&0x1b));
    assert!(!String::from_utf8_lossy(&stdout).contains('╭'));

    let (status, stdout, stderr) = run_bit_mail(
        directory.path(),
        &["status", "--all-accounts"],
        40,
        "xterm",
        false,
        false,
        true,
    )
    .unwrap();
    assert!(status.success());
    assert!(!stdout.contains(&0x1b));
    assert!(String::from_utf8_lossy(&stdout).starts_with("personal\tpending="));
    assert!(stderr.is_empty());

    let (status, stdout, stderr) = run_bit_mail(
        directory.path(),
        &["--account", "missing", "status"],
        40,
        "xterm",
        false,
        true,
        false,
    )
    .unwrap();
    assert!(!status.success());
    assert!(stdout.is_empty());
    assert!(!stderr.contains(&0x1b));
}

const PROGRESS_HELPER: &str = "BIT_MAIL_PROGRESS_HELPER";

#[test]
fn active_progress_preserves_a_concurrent_diagnostic() {
    if env::var_os(PROGRESS_HELPER).is_some() {
        let spinner = bit_mail::progress::Spinner::new(true);
        spinner.report(bit_mail::progress::Event::Phase("Working".into()));
        thread::sleep(Duration::from_millis(500));
        writeln!(bit_mail::progress::stderr_writer(), "diagnostic").unwrap();
        spinner.report(bit_mail::progress::Event::Suspend);
        return;
    }

    let executable = env::current_exe().unwrap();
    let (status, _, stderr) = run_pty(
        &executable,
        Path::new(env!("CARGO_MANIFEST_DIR")),
        &[
            "--exact",
            "active_progress_preserves_a_concurrent_diagnostic",
            "--nocapture",
        ],
        80,
        "xterm",
        false,
        false,
        true,
        Some((PROGRESS_HELPER, "1")),
    )
    .unwrap();
    assert!(status.success());
    let stderr = String::from_utf8(stderr).unwrap();
    assert!(stderr.contains("Working"), "{stderr:?}");
    assert!(stderr.contains("\x1b[2Kdiagnostic"), "{stderr:?}");
}
