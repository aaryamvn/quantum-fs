use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use clap::Parser;
use quantam_fs::{
    config::Config,
    daemon::{member_role, MemberRole},
    ids::PeerId,
    keystore::{load_or_create_identity_path, IdentityState},
};

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = std::env::temp_dir().join(format!("qfsd-test-{}-{unique}", std::process::id()));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn help_lists_daemon_options() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_qfsd"))
        .arg("--help")
        .output()?;
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout)?;
    for option in [
        "--data-dir",
        "--listen-addr",
        "--peer-identity-path",
        "--host-id",
    ] {
        assert!(help.contains(option));
    }
    Ok(())
}

#[test]
fn config_resolves_identity_and_reads_raw_host_id() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TestDir::new()?;
    let host_path = dir.0.join("host-id");
    fs::write(&host_path, [0x80; 32])?;
    let config = Config::try_parse_from([
        "qfsd",
        "--data-dir",
        dir.0.to_str().ok_or("temp path is not UTF-8")?,
        "--peer-identity-path",
        "keys/identity",
        "--listen-addr",
        "127.0.0.1:9000",
        "--host-id",
        host_path.to_str().ok_or("temp path is not UTF-8")?,
    ])?;
    assert_eq!(config.identity_path(), dir.0.join("keys/identity"));
    assert_eq!(config.listen_addr.port(), 9000);
    assert!(config.load_host_id()? == Some(PeerId([0x80; 32])));
    fs::write(host_path, [0; 31])?;
    assert!(config.load_host_id().is_err());
    assert!(Config::try_parse_from(["qfsd", "--listen-addr", "invalid"]).is_err());
    Ok(())
}

#[test]
fn identity_placeholder_is_empty_and_existing_bytes_are_preserved(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = TestDir::new()?;
    let path = dir.0.join("keys/identity");
    assert_eq!(
        load_or_create_identity_path(&path)?.state,
        IdentityState::Pending
    );
    assert!(fs::read(&path)?.is_empty());
    fs::write(&path, b"unverified existing identity")?;
    assert_eq!(
        load_or_create_identity_path(&path)?.state,
        IdentityState::ExistingUnverified
    );
    assert_eq!(fs::read(&path)?, b"unverified existing identity");
    assert!(load_or_create_identity_path(&dir.0).is_err());
    Ok(())
}

#[test]
fn appointed_host_is_a_member_role() {
    let local = PeerId([0x80; 32]);
    assert_eq!(member_role(&local, Some(&local)), MemberRole::Host);
    assert_eq!(
        member_role(&local, Some(&PeerId([0x81; 32]))),
        MemberRole::Member
    );
    assert_eq!(member_role(&local, None), MemberRole::Member);
}
