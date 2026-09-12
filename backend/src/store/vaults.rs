use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::ErrorKind,
    path::{Path, PathBuf},
};

use crate::{
    encoding,
    ids::FileId,
    keystore::{atomic_private_write, read_private, KeyStore},
    net::VaultId,
    Error, Result,
};

const MARKER: &str = ".migrate-vaults";

pub fn vault_directory(data_dir: &Path, vault_id: VaultId) -> PathBuf {
    data_dir.join("vaults").join(hex(&vault_id.0))
}

pub(crate) fn prepare_vault_directory(
    keys: &KeyStore,
    data_dir: &Path,
    vault_id: VaultId,
) -> Result<PathBuf> {
    keys.require_data_dir_lock(data_dir)?;
    let root = data_dir.join("vaults");
    ensure_private_directory(&root)?;
    let destination = vault_directory(data_dir, vault_id);
    ensure_private_directory(&destination)?;
    keys.require_data_dir_lock(&destination)?;
    Ok(destination)
}

pub fn discover_vaults(keys: &KeyStore, data_dir: &Path) -> Result<Vec<(VaultId, PathBuf)>> {
    keys.require_data_dir_lock(data_dir)?;
    crate::store::transaction::recover_admission_transaction(&data_dir.join("vault"))?;
    resume_migration(keys, data_dir)?;
    let mut vaults = Vec::new();
    let mut ids = BTreeSet::new();
    let legacy = data_dir.join("vault");
    if let Some(bytes) = read_private(&legacy)? {
        let metadata = encoding::decode_vault_metadata(&bytes)?;
        verify_replica(&data_dir.join("replica.bin"), metadata.vault_id)?;
        ids.insert(metadata.vault_id);
        vaults.push((metadata.vault_id, data_dir.to_owned()));
    }
    let root = data_dir.join("vaults");
    if root.try_exists()? {
        require_real_directory(&root)?;
    }
    match fs::read_dir(&root) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    return Err(Error::InvalidInput("vaults contains a non-directory entry"));
                }
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| Error::InvalidInput("invalid vault directory name"))?;
                let id = parse_hex_vault_id(&name)?;
                let directory = entry.path();
                crate::store::transaction::recover_admission_transaction(&directory.join("vault"))?;
                let metadata = read_private(&directory.join("vault"))?
                    .ok_or(Error::State("vault admission file is missing"))
                    .and_then(|bytes| encoding::decode_vault_metadata(&bytes))?;
                if metadata.vault_id != id {
                    return Err(Error::State(
                        "vault directory id differs from admission file",
                    ));
                }
                if !ids.insert(id) {
                    return Err(Error::State("duplicate vault id on disk"));
                }
                verify_replica(&directory.join("replica.bin"), id)?;
                vaults.push((id, directory));
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    vaults.sort_by_key(|(id, _)| id.0);
    Ok(vaults)
}

pub fn migrate_legacy(keys: &KeyStore, data_dir: &Path) -> Result<()> {
    keys.require_data_dir_lock(data_dir)?;
    crate::store::transaction::recover_admission_transaction(&data_dir.join("vault"))?;
    let marker = data_dir.join(MARKER);
    if marker.try_exists()? {
        return resume_migration(keys, data_dir);
    }
    let Some(admission) = read_private(&data_dir.join("vault"))? else {
        return Ok(());
    };
    let vault_id = encoding::decode_vault_metadata(&admission)?.vault_id;
    verify_replica(&data_dir.join("replica.bin"), vault_id)?;
    prepare_vault_directory(keys, data_dir, vault_id)?;
    atomic_private_write(&marker, &encoding::encode_vault_migration(vault_id, 0)?)?;
    resume_migration(keys, data_dir)
}

pub fn resume_migration(keys: &KeyStore, data_dir: &Path) -> Result<()> {
    keys.require_data_dir_lock(data_dir)?;
    crate::store::transaction::recover_admission_transaction(&data_dir.join("vault"))?;
    let marker_path = data_dir.join(MARKER);
    let Some(marker) = read_private(&marker_path)? else {
        return Ok(());
    };
    let (vault_id, mut phase) = encoding::decode_vault_migration(&marker)?;
    let destination = prepare_vault_directory(keys, data_dir, vault_id)?;
    crate::store::transaction::recover_admission_transaction(&destination.join("vault"))?;
    let replica_source = data_dir.join("replica.bin");
    let replica_target = destination.join("replica.bin");
    let verify_at = if replica_source.try_exists()? {
        &replica_source
    } else {
        &replica_target
    };
    verify_replica(verify_at, vault_id)?;

    while phase < 3 {
        let name = match phase {
            0 => "replica.bin",
            1 => "chunks",
            2 => "vault",
            _ => unreachable!(),
        };
        let source = data_dir.join(name);
        let target = destination.join(name);
        move_once(&source, &target)?;
        phase += 1;
        atomic_private_write(
            &marker_path,
            &encoding::encode_vault_migration(vault_id, phase)?,
        )?;
    }
    remove_and_sync(&marker_path, data_dir)
}

fn verify_replica(path: &Path, vault_id: VaultId) -> Result<()> {
    let bytes = read_private(path)?.ok_or(Error::State("vault replica is missing"))?;
    encoding::decode_replica(&bytes, FileId(vault_id.0))?;
    Ok(())
}

fn move_once(source: &Path, target: &Path) -> Result<()> {
    let source_exists = source.try_exists()?;
    let target_exists = target.try_exists()?;
    match (source_exists, target_exists) {
        (true, false) => {
            fs::rename(source, target)?;
            sync_directory(source.parent().unwrap_or(Path::new(".")))?;
            sync_directory(target.parent().unwrap_or(Path::new(".")))
        }
        (false, true) => {
            sync_directory(source.parent().unwrap_or(Path::new(".")))?;
            sync_directory(target.parent().unwrap_or(Path::new(".")))
        }
        (true, true) => Err(Error::State(
            "vault migration source and destination both exist",
        )),
        (false, false) => Err(Error::State(
            "vault migration source and destination are missing",
        )),
    }
}

fn ensure_private_directory(path: &Path) -> Result<()> {
    let created = match fs::symlink_metadata(path) {
        Ok(_) => {
            require_real_directory(path)?;
            false
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            fs::create_dir(path)?;
            true
        }
        Err(error) => return Err(error.into()),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    if created {
        sync_directory(path.parent().unwrap_or(Path::new(".")))?;
    }
    Ok(())
}

fn require_real_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::InvalidInput("vault path must be a real directory"));
    }
    Ok(())
}

fn remove_and_sync(path: &Path, parent: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => sync_directory(parent),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)?.sync_all().map_err(Into::into)
}

fn hex(bytes: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(64);
    for byte in bytes {
        value.push(DIGITS[(byte >> 4) as usize] as char);
        value.push(DIGITS[(byte & 15) as usize] as char);
    }
    value
}

fn parse_hex_vault_id(value: &str) -> Result<VaultId> {
    if value.len() != 64 {
        return Err(Error::InvalidInput("invalid vault directory name"));
    }
    let mut bytes = [0; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = (hex_digit(value.as_bytes()[index * 2])? << 4)
            | hex_digit(value.as_bytes()[index * 2 + 1])?;
    }
    Ok(VaultId(bytes))
}

fn hex_digit(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(Error::InvalidInput("invalid vault directory name")),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;
    use crate::{
        ids::PeerId,
        net::{join::VaultMetadata, JoinCode},
        store::replica::ReplicaMetadata,
    };

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

    fn directory(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "qfs-vault-migration-{name}-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn write_legacy(data_dir: &Path, vault_id: VaultId) -> Result<()> {
        let admission = VaultMetadata {
            vault_id,
            join_code: JoinCode([3; 16]),
            issued_at: 4,
            members: vec![PeerId([5; 32])],
            denied: BTreeSet::new(),
        };
        atomic_private_write(
            &data_dir.join("vault"),
            &encoding::encode_vault_metadata(&admission)?,
        )?;
        atomic_private_write(
            &data_dir.join("replica.bin"),
            &encoding::encode_replica(&ReplicaMetadata::new(FileId(vault_id.0)), 1)?,
        )?;
        ensure_private_directory(&data_dir.join("chunks"))
    }

    #[test]
    fn resumes_when_chunks_rename_precedes_phase_advance() -> Result<()> {
        let data_dir = directory("resume-chunks");
        fs::create_dir(&data_dir)?;
        let identity = data_dir.join("identity");
        let vault_id = VaultId([0x41; 32]);
        let result = (|| -> Result<()> {
            let keys = KeyStore::open(&identity)?;
            write_legacy(&data_dir, vault_id)?;
            atomic_private_write(&data_dir.join("chunks").join("sentinel"), b"plaintext")?;
            let destination = vault_directory(&data_dir, vault_id);
            ensure_private_directory(&data_dir.join("vaults"))?;
            ensure_private_directory(&destination)?;
            fs::rename(
                data_dir.join("replica.bin"),
                destination.join("replica.bin"),
            )?;
            fs::rename(data_dir.join("chunks"), destination.join("chunks"))?;
            atomic_private_write(
                &data_dir.join(MARKER),
                &encoding::encode_vault_migration(vault_id, 1)?,
            )?;
            drop(keys);

            let keys = KeyStore::open(&identity)?;
            let discovered = discover_vaults(&keys, &data_dir)?;
            assert_eq!(discovered, vec![(vault_id, destination.clone())]);
            assert_eq!(
                fs::read(destination.join("chunks").join("sentinel"))?,
                b"plaintext"
            );
            assert!(destination.join("vault").exists());
            assert!(!data_dir.join("replica.bin").exists());
            assert!(!data_dir.join("chunks").exists());
            assert!(!data_dir.join("vault").exists());
            assert!(!data_dir.join(MARKER).exists());
            Ok(())
        })();
        let _ = fs::remove_dir_all(&data_dir);
        result
    }

    #[test]
    fn corrupt_replica_stops_before_migration_marker_or_directories() -> Result<()> {
        let data_dir = directory("corrupt");
        fs::create_dir(&data_dir)?;
        let result = (|| -> Result<()> {
            let keys = KeyStore::open(&data_dir.join("identity"))?;
            let vault_id = VaultId([0x42; 32]);
            write_legacy(&data_dir, vault_id)?;
            atomic_private_write(&data_dir.join("replica.bin"), b"corrupt")?;
            assert!(migrate_legacy(&keys, &data_dir).is_err());
            assert!(!data_dir.join(MARKER).exists());
            assert!(!data_dir.join("vaults").exists());
            assert!(data_dir.join("vault").exists());
            assert!(data_dir.join("replica.bin").exists());
            assert!(data_dir.join("chunks").exists());
            Ok(())
        })();
        let _ = fs::remove_dir_all(&data_dir);
        result
    }

    #[cfg(unix)]
    #[test]
    fn migration_rejects_destination_symlink_before_marker_or_moves() -> Result<()> {
        use std::os::unix::{fs::symlink, fs::PermissionsExt};

        let data_dir = directory("migration-symlink-root");
        let outside = directory("migration-symlink-outside");
        fs::create_dir(&data_dir)?;
        fs::create_dir(&outside)?;
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o755))?;
        let result = (|| -> Result<()> {
            let keys = KeyStore::open(&data_dir.join("identity"))?;
            let vault_id = VaultId([0x44; 32]);
            write_legacy(&data_dir, vault_id)?;
            fs::create_dir(data_dir.join("vaults"))?;
            let destination = vault_directory(&data_dir, vault_id);
            symlink(&outside, &destination)?;

            assert!(migrate_legacy(&keys, &data_dir).is_err());
            assert!(!data_dir.join(MARKER).exists());
            assert!(data_dir.join("replica.bin").exists());
            assert!(data_dir.join("chunks").exists());
            assert!(data_dir.join("vault").exists());
            assert_eq!(fs::read_dir(&outside)?.count(), 0);
            assert_eq!(fs::metadata(&outside)?.permissions().mode() & 0o777, 0o755);
            Ok(())
        })();
        let _ = fs::remove_dir_all(&data_dir);
        let _ = fs::remove_dir_all(&outside);
        result
    }

    #[cfg(unix)]
    #[test]
    fn keystore_lock_rejects_vault_symlink_escape() -> Result<()> {
        use std::os::unix::fs::symlink;

        let data_dir = directory("symlink-root");
        let outside = directory("symlink-outside");
        fs::create_dir(&data_dir)?;
        fs::create_dir(&outside)?;
        let result = (|| -> Result<()> {
            let keys = KeyStore::open(&data_dir.join("identity"))?;
            fs::create_dir(data_dir.join("vaults"))?;
            let vault_id = VaultId([0x43; 32]);
            let linked = vault_directory(&data_dir, vault_id);
            symlink(&outside, &linked)?;
            assert!(keys.require_data_dir_lock(&linked).is_err());
            Ok(())
        })();
        let _ = fs::remove_dir_all(&data_dir);
        let _ = fs::remove_dir_all(&outside);
        result
    }
}
