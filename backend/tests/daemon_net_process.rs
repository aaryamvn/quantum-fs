#![cfg(unix)]

use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path =
            std::env::temp_dir().join(format!("qfsd-net-process-{}-{unique}", std::process::id()));
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

    fn wait_for(&mut self, marker: &str) -> Result<String, Box<dyn std::error::Error>> {
        if let Some(line) = self.lines.iter().find(|line| line.contains(marker)) {
            return Ok(line.clone());
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
            let found = line.contains(marker);
            self.lines.push(line.clone());
            if found {
                return Ok(line);
            }
        }
    }

    fn terminate(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let status = Command::new("kill")
            .args(["-TERM", &self.child.id().to_string()])
            .status()?;
        if !status.success() {
            return Err("failed to send SIGTERM to qfsd".into());
        }
        self.wait_for("qfsd: shutdown complete")?;
        let status = self.child.wait()?;
        if !status.success() {
            return Err(format!("qfsd exited unsuccessfully: {status}").into());
        }
        Ok(())
    }
}

impl Drop for DaemonProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn final_word(line: &str) -> Result<&str, Box<dyn std::error::Error>> {
    line.split_whitespace()
        .next_back()
        .ok_or_else(|| "qfsd readiness line was empty".into())
}

#[test]
fn three_processes_publish_join_and_shutdown_cleanly() -> Result<(), Box<dyn std::error::Error>> {
    let root = TestDir::new()?;
    let directory_path = root.0.join("directory");
    let host_path = root.0.join("host");
    let member_path = root.0.join("member");

    let mut directory = DaemonProcess::spawn(
        &directory_path,
        &["--directory", "--listen-addr", "127.0.0.1:0"],
    )?;
    let directory_line = directory.wait_for("qfsd: directory listening")?;
    let directory_addr = final_word(&directory_line)?.to_owned();
    assert!(!directory_path.join("identity").exists());

    let mut host = DaemonProcess::spawn(
        &host_path,
        &[
            "--create-vault",
            "--directory-addr",
            &directory_addr,
            "--listen-addr",
            "127.0.0.1:0",
        ],
    )?;
    let join_line = host.wait_for("qfsd: join code")?;
    let join_code = final_word(&join_line)?.to_owned();
    host.wait_for("qfsd: vault listening")?;

    let mut member = DaemonProcess::spawn(
        &member_path,
        &[
            "--join-code",
            &join_code,
            "--directory-addr",
            &directory_addr,
            "--listen-addr",
            "127.0.0.1:0",
        ],
    )?;
    member.wait_for("qfsd: joined vault")?;
    let hosted = fs::read_dir(host_path.join("vaults"))?.collect::<std::io::Result<Vec<_>>>()?;
    assert_eq!(hosted.len(), 1);
    assert!(hosted[0].path().join("vault").is_file());
    assert!(member_path.join("identity").is_file());
    assert!(!directory_path.join("identity").exists());

    member.terminate()?;
    host.terminate()?;
    directory.terminate()?;
    Ok(())
}

#[test]
fn create_adds_a_vault_and_restart_serves_both_codes() -> Result<(), Box<dyn std::error::Error>> {
    let root = TestDir::new()?;
    let directory_path = root.0.join("directory");
    let host_path = root.0.join("host");
    let mut directory = DaemonProcess::spawn(
        &directory_path,
        &["--directory", "--listen-addr", "127.0.0.1:0"],
    )?;
    let directory_line = directory.wait_for("qfsd: directory listening")?;
    let directory_addr = final_word(&directory_line)?.to_owned();
    let create_args = [
        "--create-vault",
        "--directory-addr",
        &directory_addr,
        "--listen-addr",
        "127.0.0.1:0",
    ];
    let mut first = DaemonProcess::spawn(&host_path, &create_args)?;
    let code_a = final_word(&first.wait_for("qfsd: join code")?)?.to_owned();
    first.wait_for("qfsd: vault listening")?;
    first.terminate()?;
    let mut second = DaemonProcess::spawn(&host_path, &create_args)?;
    let code_b = final_word(&second.wait_for("qfsd: join code")?)?.to_owned();
    second.wait_for("qfsd: vault listening")?;
    assert_ne!(code_a, code_b);
    assert_eq!(fs::read_dir(host_path.join("vaults"))?.count(), 2);
    second.terminate()?;
    let mut restarted = DaemonProcess::spawn(
        &host_path,
        &[
            "--directory-addr",
            &directory_addr,
            "--listen-addr",
            "127.0.0.1:0",
        ],
    )?;
    restarted.wait_for("qfsd: vault listening")?;
    assert!(!restarted
        .lines
        .iter()
        .any(|line| line.contains("qfsd: join code")));
    let mut a = DaemonProcess::spawn(
        &root.0.join("a"),
        &[
            "--join-code",
            &code_a,
            "--directory-addr",
            &directory_addr,
            "--listen-addr",
            "127.0.0.1:0",
        ],
    )?;
    let mut b = DaemonProcess::spawn(
        &root.0.join("b"),
        &[
            "--join-code",
            &code_b,
            "--directory-addr",
            &directory_addr,
            "--listen-addr",
            "127.0.0.1:0",
        ],
    )?;
    a.wait_for("qfsd: joined vault")?;
    b.wait_for("qfsd: joined vault")?;
    a.terminate()?;
    b.terminate()?;
    restarted.terminate()?;
    directory.terminate()?;
    Ok(())
}
