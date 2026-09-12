use std::{
    fs::{self, File},
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
};

use crate::{
    encoding,
    keystore::{atomic_private_write, atomic_private_write_bounded},
    Error, Result,
};

const JOURNAL_NAME: &str = ".admission-transaction";
const MAX_ADMISSION_BYTES: u64 = 1024 * 1024;
const MAX_JOURNAL_BYTES: u64 = 17 * 1024 * 1024 + 256;

pub(crate) fn persist(
    replica_path: Option<&Path>,
    admission_path: &Path,
    replica_bytes: Option<&[u8]>,
    admission_bytes: &[u8],
) -> Result<()> {
    if replica_path.is_some() != replica_bytes.is_some() {
        return Err(Error::InvalidInput(
            "replica transaction path and bytes must be paired",
        ));
    }
    if replica_path.is_none() {
        let old_admission = read_bounded(admission_path, MAX_ADMISSION_BYTES)?.unwrap_or_default();
        if let Err(error) = atomic_private_write(admission_path, admission_bytes) {
            restore(admission_path, &old_admission)?;
            return Err(error);
        }
        return Ok(());
    }
    persist_inner(
        replica_path,
        admission_path,
        replica_bytes,
        admission_bytes,
        false,
    )
}

fn persist_inner(
    replica_path: Option<&Path>,
    admission_path: &Path,
    replica_bytes: Option<&[u8]>,
    admission_bytes: &[u8],
    fail_before_replica: bool,
) -> Result<()> {
    if admission_bytes.len() as u64 > MAX_ADMISSION_BYTES {
        return Err(Error::InvalidInput("vault admission file is too large"));
    }
    let old_replica = match replica_path {
        Some(path) => read_bounded(path, 16 * 1024 * 1024)?.unwrap_or_default(),
        None => Vec::new(),
    };
    let old_admission = read_bounded(admission_path, MAX_ADMISSION_BYTES)?.unwrap_or_default();
    let journal = encoding::encode_admission_transaction(&old_replica, &old_admission)?;
    let journal_path = journal_path(admission_path);

    atomic_private_write_bounded(&journal_path, &journal, MAX_JOURNAL_BYTES)?;
    let result = (|| {
        atomic_private_write(admission_path, admission_bytes)?;
        if let (Some(path), Some(bytes)) = (replica_path, replica_bytes) {
            if fail_before_replica {
                return Err(Error::State("injected replica transaction failure"));
            }
            atomic_private_write(path, bytes)?;
        }
        clear_journal(&journal_path)
    })();
    if let Err(error) = result {
        atomic_private_write_bounded(&journal_path, &journal, MAX_JOURNAL_BYTES)?;
        restore(admission_path, &old_admission)?;
        if replica_path.is_some() {
            let replica_path = admission_path
                .parent()
                .unwrap_or(Path::new("."))
                .join("replica.bin");
            restore(&replica_path, &old_replica)?;
        }
        clear_journal(&journal_path)?;
        return Err(error);
    }
    Ok(())
}

/// Rolls back an interrupted admission/replica publication before either file is read.
pub fn recover_admission_transaction(admission_path: &Path) -> Result<()> {
    let journal_path = journal_path(admission_path);
    let Some(bytes) = read_bounded(&journal_path, MAX_JOURNAL_BYTES)? else {
        return Ok(());
    };
    let (old_replica, old_admission) = encoding::decode_admission_transaction(&bytes)?;
    restore(admission_path, &old_admission)?;
    let replica_path = admission_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("replica.bin");
    restore(&replica_path, &old_replica)?;
    clear_journal(&journal_path)
}

fn journal_path(admission_path: &Path) -> PathBuf {
    let parent = admission_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    parent.join(JOURNAL_NAME)
}

fn restore(path: &Path, bytes: &[u8]) -> Result<()> {
    if !bytes.is_empty() {
        return atomic_private_write(path, bytes);
    }
    match fs::remove_file(path) {
        Ok(()) => sync_parent(path),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn clear_journal(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => sync_parent(path),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn sync_parent(path: &Path) -> Result<()> {
    File::open(path.parent().unwrap_or(Path::new(".")))?
        .sync_all()
        .map_err(Into::into)
}

fn read_bounded(path: &Path, maximum: u64) -> Result<Option<Vec<u8>>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() || metadata.len() > maximum {
        return Err(Error::InvalidInput("invalid admission transaction file"));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(path)?
        .take(maximum + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        return Err(Error::InvalidInput(
            "admission transaction file is too large",
        ));
    }
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

    fn directory(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "qfs-admission-transaction-{name}-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn publishes_both_files_and_clears_journal() -> Result<()> {
        let directory = directory("commit");
        fs::create_dir(&directory)?;
        let admission = directory.join("vault");
        let replica = directory.join("replica.bin");
        atomic_private_write(&admission, b"old admission")?;
        atomic_private_write(&replica, b"old replica")?;
        persist(
            Some(&replica),
            &admission,
            Some(b"new replica"),
            b"new admission",
        )?;
        assert_eq!(fs::read(&admission)?, b"new admission");
        assert_eq!(fs::read(&replica)?, b"new replica");
        assert!(!directory.join(JOURNAL_NAME).exists());
        fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[test]
    fn second_file_failure_rolls_back_both_files() -> Result<()> {
        let directory = directory("second-write");
        fs::create_dir(&directory)?;
        let admission = directory.join("vault");
        let replica = directory.join("replica.bin");
        atomic_private_write(&admission, b"old admission")?;
        atomic_private_write(&replica, b"old replica")?;
        assert!(persist_inner(
            Some(&replica),
            &admission,
            Some(b"new replica"),
            b"new admission",
            true
        )
        .is_err());
        assert_eq!(fs::read(&admission)?, b"old admission");
        assert_eq!(fs::read(&replica)?, b"old replica");
        assert!(!directory.join(JOURNAL_NAME).exists());
        fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[test]
    fn startup_recovery_rolls_back_a_partial_publication() -> Result<()> {
        let directory = directory("recovery");
        fs::create_dir(&directory)?;
        let admission = directory.join("vault");
        let replica = directory.join("replica.bin");
        atomic_private_write(&admission, b"old admission")?;
        atomic_private_write(&replica, b"old replica")?;
        let journal = encoding::encode_admission_transaction(b"old replica", b"old admission")?;
        atomic_private_write_bounded(&directory.join(JOURNAL_NAME), &journal, MAX_JOURNAL_BYTES)?;
        atomic_private_write(&admission, b"new admission")?;
        recover_admission_transaction(&admission)?;
        assert_eq!(fs::read(&admission)?, b"old admission");
        assert_eq!(fs::read(&replica)?, b"old replica");
        assert!(!directory.join(JOURNAL_NAME).exists());
        fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[test]
    fn startup_recovery_removes_first_uncommitted_replica() -> Result<()> {
        let directory = directory("first-replica-recovery");
        fs::create_dir(&directory)?;
        let admission = directory.join("vault");
        let replica = directory.join("replica.bin");
        atomic_private_write(&admission, b"old admission")?;
        let journal = encoding::encode_admission_transaction(&[], b"old admission")?;
        atomic_private_write_bounded(&directory.join(JOURNAL_NAME), &journal, MAX_JOURNAL_BYTES)?;
        atomic_private_write(&admission, b"new admission")?;
        atomic_private_write(&replica, b"first replica")?;

        recover_admission_transaction(&admission)?;
        assert_eq!(fs::read(&admission)?, b"old admission");
        assert!(!replica.exists());
        assert!(!directory.join(JOURNAL_NAME).exists());
        fs::remove_dir_all(directory)?;
        Ok(())
    }
}
