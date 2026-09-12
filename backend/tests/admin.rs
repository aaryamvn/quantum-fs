#![cfg(unix)]

use std::{
    fs,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use quantam_fs::net::short_code;

type Outcome = Result<(), Box<dyn std::error::Error>>;

struct TestDir(PathBuf);

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

impl TestDir {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let seq = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("qfsd-admin-{}-{unique}-{seq}", std::process::id()));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct DaemonProcess {
    child: Child,
    lines: Vec<String>,
    receiver: Receiver<String>,
}

impl DaemonProcess {
    fn spawn(data_dir: &Path, arguments: &[&str]) -> Result<Self, Box<dyn std::error::Error>> {
        let mut command = Command::new(env!("CARGO_BIN_EXE_qfsd"));
        command
            .arg("--data-dir")
            .arg(data_dir)
            .args(arguments)
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = command.spawn()?;
        let stderr = child.stderr.take().ok_or("qfsd stderr was not piped")?;
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                match line {
                    Ok(line) => {
                        if sender.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            child,
            lines: Vec::new(),
            receiver,
        })
    }

    /// Waits for the plain stderr line that carries text after `marker`; the
    /// demo-log block prints the same headline with nothing following it.
    fn wait_for_value(&mut self, marker: &str) -> Result<String, Box<dyn std::error::Error>> {
        let found = |line: &str| {
            line.split_once(marker)
                .map(|(_, rest)| rest.trim().to_owned())
                .filter(|rest| !rest.is_empty())
        };
        if let Some(value) = self.lines.iter().find_map(|line| found(line)) {
            return Ok(value);
        }
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(format!(
                    "timed out waiting for {marker:?}; qfsd output: {:?}",
                    self.lines
                )
                .into());
            }
            let line = self.receiver.recv_timeout(remaining)?;
            self.lines.push(line.clone());
            if let Some(value) = found(&line) {
                return Ok(value);
            }
        }
    }

    fn terminate(&mut self) -> Outcome {
        let status = Command::new("kill")
            .args(["-TERM", &self.child.id().to_string()])
            .status()?;
        if !status.success() {
            return Err("failed to send SIGTERM to qfsd".into());
        }
        let _ = self.child.wait()?;
        Ok(())
    }
}

impl Drop for DaemonProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Reserves a loopback port by binding and dropping: the daemon then binds it
/// at a known number, which an ephemeral listener could not report back.
fn free_port() -> Result<u16, Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

struct Admin {
    reader: BufReader<TcpStream>,
}

impl Admin {
    fn connect(addr: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match TcpStream::connect(addr) {
                Ok(stream) => {
                    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
                    return Ok(Self {
                        reader: BufReader::new(stream),
                    });
                }
                Err(error) if Instant::now() < deadline => {
                    let _ = error;
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn send(&mut self, command: &str) -> Outcome {
        let stream = self.reader.get_mut();
        stream.write_all(format!("{command}\n").as_bytes())?;
        stream.flush()?;
        Ok(())
    }

    fn line(&mut self) -> Result<String, Box<dyn std::error::Error>> {
        let mut line = String::new();
        if self.reader.read_line(&mut line)? == 0 {
            return Err("admin port closed the connection".into());
        }
        Ok(line.trim_end().to_owned())
    }

    fn ask(&mut self, command: &str) -> Result<String, Box<dyn std::error::Error>> {
        self.send(command)?;
        self.line()
    }

    fn block(&mut self, command: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        self.send(command)?;
        let mut lines = Vec::new();
        loop {
            let line = self.line()?;
            if line == "END" {
                return Ok(lines);
            }
            if line.starts_with("ERR ") {
                return Err(format!("admin command failed: {line}").into());
            }
            lines.push(line);
            if lines.len() > 512 {
                return Err("admin block did not terminate".into());
            }
        }
    }
}

#[test]
fn admin_port_serves_status_create_rotate_kick_and_forget() -> Outcome {
    let root = TestDir::new()?;
    let directory_port = free_port()?;
    let host_port = free_port()?;
    let admin_port = free_port()?;
    let directory_addr = format!("127.0.0.1:{directory_port}");
    let admin_addr = format!("127.0.0.1:{admin_port}");

    let mut directory = DaemonProcess::spawn(
        &root.0.join("directory"),
        &["--directory", "--listen-addr", &directory_addr],
    )?;
    directory.wait_for_value("qfsd: directory address ")?;

    let mut host = DaemonProcess::spawn(
        &root.0.join("host"),
        &[
            "--create-vault",
            "--directory-addr",
            &directory_addr,
            "--listen-addr",
            &format!("127.0.0.1:{host_port}"),
            "--admin-addr",
            &admin_addr,
            "--capacity-bytes",
            "4096",
        ],
    )?;
    let connect = host.wait_for_value("qfsd: app connect string ")?;
    let (printed_addr, token) = connect
        .split_once('/')
        .ok_or("connect string is not ADDRESS/TOKEN")?;
    assert_eq!(printed_addr, admin_addr);
    assert_eq!(token.len(), 20);

    let mut denied = Admin::connect(&admin_addr)?;
    assert_eq!(denied.ask(&format!("AUTH {token}x"))?, "ERR unauthorized");

    let mut admin = Admin::connect(&admin_addr)?;
    assert_eq!(admin.ask(&format!("AUTH {token}"))?, "OK");
    assert_eq!(admin.ask("PING")?, "OK");
    assert_eq!(admin.ask("NONSENSE")?, "ERR unknown command");

    let first = admin.block("STATUS")?;
    let server = first.first().ok_or("STATUS printed no SERVER line")?;
    let fields: Vec<&str> = server.split_whitespace().collect();
    assert_eq!(fields[0], "SERVER");
    assert_eq!(fields[1].len(), 64);
    assert_eq!(fields[2], directory_addr);
    assert_eq!(fields[4], "4096");
    assert_eq!(
        first
            .iter()
            .filter(|line| line.starts_with("VAULT "))
            .count(),
        1
    );

    let created = admin.ask("CREATE_VAULT 1048576 -")?;
    let created: Vec<&str> = created.split_whitespace().collect();
    assert_eq!(created[0], "OK");
    let vault = created[1].to_owned();
    let code = created[2].to_owned();
    assert_eq!(vault.len(), 64);
    assert_eq!(code.len(), 6);
    assert_eq!(short_code::normalize(&code).as_deref(), Some(code.as_str()));

    let second = admin.block("STATUS")?;
    let line = second
        .iter()
        .find(|line| line.starts_with(&format!("VAULT {vault} ")))
        .ok_or("created vault is missing from STATUS")?;
    let fields: Vec<&str> = line.split_whitespace().collect();
    assert_eq!(fields[2], code);
    assert_eq!(fields[3], "1048576");
    assert_eq!(fields[4], "0");
    assert_eq!(fields[5], "1");
    assert_eq!(fields[6], "1");
    assert_eq!(fields[8], "-");
    assert_eq!(
        second
            .iter()
            .filter(|line| line.starts_with(&format!("MEMBER {vault} ")))
            .count(),
        1
    );
    assert!(admin.block(&format!("OPS {vault} 0"))?.is_empty());

    let rotated = admin.ask(&format!("ROTATE_CODE {vault}"))?;
    let rotated = rotated
        .strip_prefix("OK ")
        .ok_or("ROTATE_CODE did not return a code")?
        .to_owned();
    assert_ne!(rotated, code);
    assert_eq!(rotated.len(), 6);
    let after_rotate = admin.block("STATUS")?;
    let line = after_rotate
        .iter()
        .find(|line| line.starts_with(&format!("VAULT {vault} ")))
        .ok_or("rotated vault is missing from STATUS")?;
    assert_eq!(line.split_whitespace().nth(2), Some(rotated.as_str()));

    let stranger = "11".repeat(32);
    assert!(admin
        .ask(&format!("KICK {vault} {stranger}"))?
        .starts_with("ERR "));
    assert!(admin
        .ask(&format!("KICK {vault} nothex"))?
        .starts_with("ERR "));

    assert_eq!(admin.ask(&format!("FORGET_VAULT {vault}"))?, "OK");
    let third = admin.block("STATUS")?;
    assert!(!third.iter().any(|line| line.contains(&vault)));
    assert_eq!(
        admin.ask(&format!("ROTATE_CODE {vault}"))?,
        "ERR invalid input: unknown vault"
    );
    let removed = fs::read_dir(root.0.join("host").join("vaults"))?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(".removed-"))
        .count();
    assert_eq!(removed, 1);

    host.terminate()?;
    directory.terminate()?;
    Ok(())
}

/// The six-character code a host mints derives into the 16-byte protocol code,
/// so a member joins with it; the read-only commands need no token and hide it.
#[test]
fn short_code_admits_a_member_and_reads_stay_unauthenticated() -> Outcome {
    let root = TestDir::new()?;
    let directory_port = free_port()?;
    let host_port = free_port()?;
    let member_port = free_port()?;
    let admin_port = free_port()?;
    let directory_addr = format!("127.0.0.1:{directory_port}");
    let admin_addr = format!("127.0.0.1:{admin_port}");

    let mut directory = DaemonProcess::spawn(
        &root.0.join("directory"),
        &["--directory", "--listen-addr", &directory_addr],
    )?;
    directory.wait_for_value("qfsd: directory address ")?;

    let mut host = DaemonProcess::spawn(
        &root.0.join("host"),
        &[
            "--create-vault",
            "--directory-addr",
            &directory_addr,
            "--listen-addr",
            &format!("127.0.0.1:{host_port}"),
            "--admin-addr",
            &admin_addr,
            "--capacity-bytes",
            "4096",
        ],
    )?;
    let connect = host.wait_for_value("qfsd: app connect string ")?;
    let token = connect
        .split_once('/')
        .ok_or("connect string is not ADDRESS/TOKEN")?
        .1
        .to_owned();

    let mut admin = Admin::connect(&admin_addr)?;
    assert_eq!(admin.ask(&format!("AUTH {token}"))?, "OK");
    let created = admin.ask("CREATE_VAULT 1048576 -")?;
    let created: Vec<&str> = created.split_whitespace().collect();
    let vault = created[1].to_owned();
    let short = created[2].to_owned();
    assert_eq!(short.len(), 6);

    // No AUTH at all: PING, STATUS and OPS answer, writes do not, and the
    // join code is withheld.
    let mut anonymous = Admin::connect(&admin_addr)?;
    assert_eq!(anonymous.ask("PING")?, "OK");
    assert!(anonymous.block(&format!("OPS {vault} 0"))?.is_empty());
    assert_eq!(
        anonymous.ask(&format!("ROTATE_CODE {vault}"))?,
        "ERR unauthorized"
    );
    assert_eq!(anonymous.ask("CREATE_VAULT 1 -")?, "ERR unauthorized");
    for line in anonymous.block("STATUS")? {
        if line.starts_with("VAULT ") {
            assert_eq!(line.split_whitespace().nth(2), Some("-"));
        }
    }
    assert_eq!(vault_field(&mut admin, &vault, 2)?, short);

    // The member never sees the short code: it types the 26-character code the
    // short one derives into.
    let derived = short_code::derive(&short)?.to_string();
    assert_eq!(derived.len(), 26);
    let mut member = DaemonProcess::spawn(
        &root.0.join("member"),
        &[
            "--directory-addr",
            &directory_addr,
            "--listen-addr",
            &format!("127.0.0.1:{member_port}"),
            "--join-code",
            &derived,
        ],
    )?;

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if vault_field(&mut admin, &vault, 5)? == "2" {
            break;
        }
        if Instant::now() >= deadline {
            return Err("member never appeared in the hosted vault".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(vault_field(&mut admin, &vault, 2)?, short);

    member.terminate()?;
    host.terminate()?;
    directory.terminate()?;
    Ok(())
}

fn vault_field(
    admin: &mut Admin,
    vault: &str,
    index: usize,
) -> Result<String, Box<dyn std::error::Error>> {
    let status = admin.block("STATUS")?;
    let line = status
        .iter()
        .find(|line| line.starts_with(&format!("VAULT {vault} ")))
        .ok_or("vault is missing from STATUS")?;
    Ok(line
        .split_whitespace()
        .nth(index)
        .ok_or("VAULT line is short")?
        .to_owned())
}
