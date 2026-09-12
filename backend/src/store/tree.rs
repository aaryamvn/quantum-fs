use std::collections::{BTreeMap, BTreeSet};

use crate::{ids::FileId, Error, Result};

pub const MAX_COMPONENT_BYTES: usize = 255;
pub const MAX_PATH_BYTES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dirent {
    pub parent: FileId,
    pub name: String,
    pub child: FileId,
    pub is_dir: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryTree {
    expected_root: FileId,
    entries: BTreeMap<(FileId, String), Dirent>,
}

impl DirectoryTree {
    pub fn new(expected_root: FileId) -> Self {
        let root = Dirent {
            parent: expected_root,
            name: String::new(),
            child: expected_root,
            is_dir: true,
        };
        Self {
            expected_root,
            entries: BTreeMap::from([((expected_root, String::new()), root)]),
        }
    }

    pub fn from_dirents(expected_root: FileId, dirents: Vec<Dirent>) -> Result<Self> {
        // An empty list is the one-time migration from the storage-only
        // snapshot. Every subsequent write includes the explicit root entry.
        if dirents.is_empty() {
            return Ok(Self::new(expected_root));
        }
        let mut entries = BTreeMap::new();
        let mut children = BTreeSet::new();
        let mut previous = None;
        for entry in dirents {
            let key = (entry.parent, entry.name.clone());
            if previous.as_ref().is_some_and(|old| old >= &key) {
                return Err(Error::InvalidInput("dirents are not canonically ordered"));
            }
            if entry.name.is_empty() {
                if entry.parent != expected_root || entry.child != expected_root || !entry.is_dir {
                    return Err(Error::InvalidInput("invalid root dirent"));
                }
            } else {
                validate_name(&entry.name)?;
                if entry.child == expected_root || !children.insert(entry.child) {
                    return Err(Error::InvalidInput("file id has multiple directory links"));
                }
            }
            previous = Some(key.clone());
            entries.insert(key, entry);
        }
        if !entries.contains_key(&(expected_root, String::new())) {
            return Err(Error::InvalidInput("missing root dirent"));
        }
        let tree = Self {
            expected_root,
            entries,
        };
        tree.validate_graph()?;
        Ok(tree)
    }

    pub fn dirents(&self) -> Vec<Dirent> {
        self.entries.values().cloned().collect()
    }

    pub fn is_linked_file(&self, file_id: &FileId) -> bool {
        self.entries
            .values()
            .any(|entry| entry.child == *file_id && !entry.is_dir)
    }

    pub fn is_dir(&self, file_id: &FileId) -> bool {
        *file_id == self.expected_root
            || self
                .entries
                .values()
                .any(|entry| entry.child == *file_id && entry.is_dir)
    }

    pub fn resolve(&self, path: &str) -> Result<FileId> {
        let components = path_components(path)?;
        let mut current = self.expected_root;
        for component in components {
            let entry = self
                .entries
                .get(&(current, component.to_owned()))
                .ok_or(Error::State("path does not exist"))?;
            current = entry.child;
        }
        Ok(current)
    }

    pub fn resolve_parent(&self, path: &str) -> Result<(FileId, String)> {
        let components = path_components(path)?;
        let (name, parents) = components
            .split_last()
            .ok_or(Error::InvalidInput("root has no parent"))?;
        let mut parent = self.expected_root;
        for component in parents {
            let entry = self
                .entries
                .get(&(parent, (*component).to_owned()))
                .ok_or(Error::State("parent path does not exist"))?;
            if !entry.is_dir {
                return Err(Error::State("path parent is not a directory"));
            }
            parent = entry.child;
        }
        Ok((parent, (*name).to_owned()))
    }

    pub fn link(&mut self, parent: FileId, name: &str, child: FileId, is_dir: bool) -> Result<()> {
        validate_name(name)?;
        if !self.is_dir(&parent) {
            return Err(Error::State("link parent is not a directory"));
        }
        if child == self.expected_root || self.entries.values().any(|entry| entry.child == child) {
            return Err(Error::State("file id is already linked"));
        }
        let key = (parent, name.to_owned());
        if self.entries.contains_key(&key) {
            return Err(Error::State("directory name already exists"));
        }
        let parent_len = self.path_len(parent)?;
        let separator = usize::from(parent != self.expected_root);
        if parent_len
            .checked_add(separator)
            .and_then(|length| length.checked_add(name.len()))
            .is_none_or(|length| length > MAX_PATH_BYTES)
        {
            return Err(Error::InvalidInput("path exceeds 4096 bytes"));
        }
        self.entries.insert(
            key,
            Dirent {
                parent,
                name: name.to_owned(),
                child,
                is_dir,
            },
        );
        Ok(())
    }

    pub fn unlink(&mut self, parent: FileId, name: &str) -> Result<FileId> {
        validate_name(name)?;
        let key = (parent, name.to_owned());
        let entry = self
            .entries
            .get(&key)
            .ok_or(Error::State("directory entry does not exist"))?;
        if entry.is_dir
            && self
                .entries
                .values()
                .any(|child| child.parent == entry.child)
        {
            return Err(Error::State("directory is not empty"));
        }
        let child = entry.child;
        self.entries.remove(&key);
        Ok(child)
    }

    pub fn rename(
        &mut self,
        src_parent: FileId,
        src_name: &str,
        dst_parent: FileId,
        dst_name: &str,
    ) -> Result<()> {
        validate_name(src_name)?;
        validate_name(dst_name)?;
        if !self.is_dir(&dst_parent) {
            return Err(Error::State("rename destination is not a directory"));
        }
        let source_key = (src_parent, src_name.to_owned());
        let destination_key = (dst_parent, dst_name.to_owned());
        if source_key == destination_key {
            return self
                .entries
                .contains_key(&source_key)
                .then_some(())
                .ok_or(Error::State("rename source does not exist"));
        }
        if self.entries.contains_key(&destination_key) {
            return Err(Error::State("rename destination already exists"));
        }
        let mut entry = self
            .entries
            .get(&source_key)
            .cloned()
            .ok_or(Error::State("rename source does not exist"))?;
        if entry.name.is_empty() {
            return Err(Error::InvalidInput("cannot rename the vault root"));
        }
        if entry.is_dir && self.is_descendant(dst_parent, entry.child) {
            return Err(Error::State("cannot move a directory into its descendant"));
        }
        let mut staged = self.clone();
        staged.entries.remove(&source_key);
        entry.parent = dst_parent;
        entry.name = dst_name.to_owned();
        staged.entries.insert(destination_key, entry);
        staged.validate_graph()?;
        *self = staged;
        Ok(())
    }

    pub fn remove_id(&mut self, file_id: &FileId) -> Result<()> {
        if *file_id == self.expected_root {
            return Err(Error::InvalidInput("cannot remove the vault root"));
        }
        let Some((parent, name)) = self
            .entries
            .iter()
            .find_map(|(key, entry)| (entry.child == *file_id).then(|| key.clone()))
        else {
            return Ok(());
        };
        self.unlink(parent, &name)?;
        Ok(())
    }

    fn is_descendant(&self, mut candidate: FileId, ancestor: FileId) -> bool {
        while candidate != self.expected_root {
            if candidate == ancestor {
                return true;
            }
            let Some(parent) = self
                .entries
                .values()
                .find(|entry| entry.child == candidate)
                .map(|entry| entry.parent)
            else {
                return false;
            };
            candidate = parent;
        }
        false
    }

    fn validate_graph(&self) -> Result<()> {
        for entry in self.entries.values().filter(|entry| !entry.name.is_empty()) {
            if !self.is_dir(&entry.parent) {
                return Err(Error::InvalidInput("dirent parent is not a directory"));
            }
            let mut seen = BTreeSet::new();
            let mut current = entry.child;
            while current != self.expected_root {
                if !seen.insert(current) {
                    return Err(Error::InvalidInput("directory tree contains a cycle"));
                }
                let parent = self
                    .entries
                    .values()
                    .find(|candidate| candidate.child == current)
                    .ok_or(Error::InvalidInput("directory entry is unreachable"))?
                    .parent;
                current = parent;
            }
            self.path_len(entry.child)?;
        }
        Ok(())
    }

    fn path_len(&self, mut child: FileId) -> Result<usize> {
        if child == self.expected_root {
            return Ok(0);
        }
        let mut component_bytes = 0usize;
        let mut component_count = 0usize;
        let mut seen = BTreeSet::new();
        while child != self.expected_root {
            if !seen.insert(child) {
                return Err(Error::InvalidInput("directory tree contains a cycle"));
            }
            let entry = self
                .entries
                .values()
                .find(|entry| entry.child == child)
                .ok_or(Error::InvalidInput("directory entry is unreachable"))?;
            component_bytes = component_bytes
                .checked_add(entry.name.len())
                .ok_or(Error::InvalidInput("path length overflow"))?;
            component_count += 1;
            child = entry.parent;
        }
        let length = component_bytes
            .checked_add(component_count.saturating_sub(1))
            .ok_or(Error::InvalidInput("path length overflow"))?;
        if length > MAX_PATH_BYTES {
            return Err(Error::InvalidInput("path exceeds 4096 bytes"));
        }
        Ok(length)
    }
}

fn path_components(path: &str) -> Result<Vec<&str>> {
    if path.len() > MAX_PATH_BYTES {
        return Err(Error::InvalidInput("path exceeds 4096 bytes"));
    }
    if path.contains('\0') || path.contains('\\') {
        return Err(Error::InvalidInput("path contains a forbidden character"));
    }
    let path = path.strip_prefix('/').unwrap_or(path);
    if path.starts_with('/') || has_drive_prefix(path) {
        return Err(Error::InvalidInput("path is not vault-relative"));
    }
    if path.is_empty() {
        return Ok(Vec::new());
    }
    let components: Vec<_> = path.split('/').collect();
    for component in &components {
        validate_name(component)?;
    }
    Ok(components)
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || name == "." || name == ".." {
        return Err(Error::InvalidInput("invalid empty or dot path component"));
    }
    if name.len() > MAX_COMPONENT_BYTES {
        return Err(Error::InvalidInput("path component exceeds 255 bytes"));
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') || has_drive_prefix(name) {
        return Err(Error::InvalidInput(
            "path component contains a forbidden character",
        ));
    }
    Ok(())
}

fn has_drive_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_strict_byte_exact_and_case_sensitive() -> Result<()> {
        let root = FileId([1; 32]);
        let mut tree = DirectoryTree::new(root);
        tree.link(root, "é", FileId([2; 32]), false)?;
        tree.link(root, "e\u{301}", FileId([3; 32]), false)?;
        tree.link(root, "A", FileId([4; 32]), false)?;
        tree.link(root, "a", FileId([5; 32]), false)?;
        assert_ne!(tree.resolve("/é")?, tree.resolve("e\u{301}")?);
        assert_ne!(tree.resolve("A")?, tree.resolve("a")?);
        for invalid in ["../x", "a//b", "a/./b", "a/../b", "a\\b", "C:/x", "a\0b"] {
            assert!(tree.resolve(invalid).is_err());
        }
        assert!(tree.link(root, "C:", FileId([6; 32]), false).is_err());
        Ok(())
    }

    #[test]
    fn directory_unlink_and_descendant_rename_fail() -> Result<()> {
        let root = FileId([10; 32]);
        let a = FileId([11; 32]);
        let b = FileId([12; 32]);
        let mut tree = DirectoryTree::new(root);
        tree.link(root, "a", a, true)?;
        tree.link(a, "b", b, true)?;
        assert!(tree.unlink(root, "a").is_err());
        assert!(tree.rename(root, "a", b, "a").is_err());
        tree.unlink(a, "b")?;
        assert_eq!(tree.unlink(root, "a")?, a);
        Ok(())
    }

    #[test]
    fn raw_tree_operations_enforce_full_path_limit() -> Result<()> {
        let root = FileId([20; 32]);
        let mut tree = DirectoryTree::new(root);
        let component = "x".repeat(MAX_COMPONENT_BYTES);
        let mut parent = root;
        for value in 21..=36 {
            let child = FileId([value; 32]);
            tree.link(parent, &component, child, true)?;
            parent = child;
        }
        let before = tree.dirents();
        assert!(tree
            .link(parent, &component, FileId([37; 32]), true)
            .is_err());
        assert_eq!(tree.dirents(), before);
        Ok(())
    }
}
