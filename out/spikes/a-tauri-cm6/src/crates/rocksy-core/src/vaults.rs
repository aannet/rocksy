//! Registry of the vaults known to Rocksy (`vaults.json` in the application
//! data directory). A folder is a vault only if it is listed here; nothing is
//! ever written into the vault itself to mark it.

use crate::atomic::write_data_file;
use crate::error::{ApiError, Result};
use crate::paths::{absolute_string, key_is_inside, last_component, path_key};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const VAULTS_FILE: &str = "vaults.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultRef {
    pub path: String,
    pub name: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RegistryFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    vaults: Vec<VaultRef>,
}

fn registry_path(data_dir: &Path) -> PathBuf {
    data_dir.join(VAULTS_FILE)
}

/// Registered vaults, in the order they were added. A missing or corrupted
/// registry is an empty list.
pub fn list_vaults(data_dir: &Path) -> Vec<VaultRef> {
    let Ok(bytes) = fs::read(registry_path(data_dir)) else {
        return Vec::new();
    };
    serde_json::from_slice::<RegistryFile>(&bytes).map(|f| f.vaults).unwrap_or_default()
}

fn write_registry(data_dir: &Path, vaults: &[VaultRef]) -> Result<()> {
    let path = registry_path(data_dir);
    // Keep a copy of a registry we could not parse instead of silently
    // destroying it.
    if let Ok(bytes) = fs::read(&path) {
        if serde_json::from_slice::<RegistryFile>(&bytes).is_err() {
            let _ = fs::write(data_dir.join(format!("{VAULTS_FILE}.corrupt")), &bytes);
        }
    }
    let file = RegistryFile { version: 1, vaults: vaults.to_vec() };
    let json = serde_json::to_vec_pretty(&file)
        .map_err(|e| ApiError::io(format!("cannot serialize the vault registry: {e}")))?;
    write_data_file(&path, &json)
}

/// Registers the folder `path` as a vault and returns the updated list.
/// Adding a vault that is already registered is a no-op. Nothing is created
/// or modified inside the vault.
///
/// Errors: `not_found` (no such folder), `invalid` (not a directory, or the
/// data directory lies inside the vault).
pub fn add_vault(data_dir: &Path, path: &Path) -> Result<Vec<VaultRef>> {
    let abs = absolute_string(path)?;
    let meta = fs::metadata(&abs).map_err(|e| ApiError::from_io(&e, format!("cannot open {abs}")))?;
    if !meta.is_dir() {
        return Err(ApiError::invalid(format!("{abs} is not a folder")));
    }
    let key = path_key(&abs);
    if let Ok(data_abs) = absolute_string(data_dir) {
        if key_is_inside(&path_key(&data_abs), &key, cfg!(windows)) {
            return Err(ApiError::invalid(format!(
                "the application data directory is inside {abs}: it cannot be a vault"
            )));
        }
    }
    let mut vaults = list_vaults(data_dir);
    if vaults.iter().any(|v| path_key(&v.path) == key) {
        return Ok(vaults);
    }
    let name = match last_component(&abs) {
        "" => abs.clone(),
        n => n.to_string(),
    };
    vaults.push(VaultRef { path: abs, name });
    write_registry(data_dir, &vaults)?;
    Ok(vaults)
}

/// Removes a vault from the registry and returns the updated list. No file
/// of the vault is touched; removing an unknown path is a no-op.
pub fn remove_vault(data_dir: &Path, path: &Path) -> Result<Vec<VaultRef>> {
    let key = path_key(&absolute_string(path)?);
    let mut vaults = list_vaults(data_dir);
    let before = vaults.len();
    vaults.retain(|v| path_key(&v.path) != key);
    if vaults.len() != before {
        write_registry(data_dir, &vaults)?;
    }
    Ok(vaults)
}

/// The vault of `vaults` that contains `path` (the deepest one when vaults
/// are nested), if any. Pure function: the file system is not consulted.
pub fn find_vault<'a>(vaults: &'a [VaultRef], path: &Path) -> Option<&'a VaultRef> {
    let key = path_key(&absolute_string(path).ok()?);
    vaults
        .iter()
        .filter(|v| key_is_inside(&key, &path_key(&v.path), cfg!(windows)))
        .max_by_key(|v| path_key(&v.path).len())
}

/// The registered vault that contains `path`, if any. A file that sits in an
/// unregistered vault folder gets `None` and opens as an isolated file.
pub fn vault_for_path(data_dir: &Path, path: &Path) -> Option<VaultRef> {
    let vaults = list_vaults(data_dir);
    find_vault(&vaults, path).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;
    use crate::testutil::{all_files, TempDir};

    #[test]
    fn add_list_remove_round_trip() {
        let dir = TempDir::new("vaults");
        let data = dir.path().join("data");
        let v1 = dir.path().join("Vault One");
        let v2 = dir.path().join("v2");
        fs::create_dir_all(&v1).unwrap();
        fs::create_dir_all(&v2).unwrap();
        fs::write(v1.join("note.md"), "x").unwrap();

        assert!(list_vaults(&data).is_empty());
        let list = add_vault(&data, &v1).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Vault One");
        assert_eq!(list[0].path, v1.to_str().unwrap());

        // Trailing separator and duplicates do not create a second entry.
        let with_slash = PathBuf::from(format!("{}/", v1.display()));
        assert_eq!(add_vault(&data, &with_slash).unwrap().len(), 1);
        let list = add_vault(&data, &v2).unwrap();
        assert_eq!(list.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["Vault One", "v2"]);
        assert_eq!(list_vaults(&data), list);

        let list = remove_vault(&data, &v1).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "v2");
        // Removing again is a no-op; the vault's files are untouched.
        assert_eq!(remove_vault(&data, &v1).unwrap().len(), 1);
        assert_eq!(all_files(&v1), vec!["note.md"]);
        assert!(all_files(&v2).is_empty());
        assert_eq!(all_files(&data), vec![VAULTS_FILE]);
    }

    #[test]
    fn add_rejects_missing_folder_file_and_data_dir_parent() {
        let dir = TempDir::new("vaults-err");
        let data = dir.path().join("vault/appdata");
        let err = add_vault(&data, &dir.path().join("nope")).unwrap_err();
        assert_eq!(err.kind, ErrorKind::NotFound);
        let f = dir.write("file.md", "x");
        let err = add_vault(&data, &f).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Invalid);
        fs::create_dir_all(dir.path().join("vault")).unwrap();
        let err = add_vault(&data, &dir.path().join("vault")).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Invalid);
        assert!(!data.exists());
    }

    #[test]
    fn corrupted_registry_is_tolerated_and_preserved() {
        let dir = TempDir::new("vaults-bad");
        let data = dir.path().join("data");
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join(VAULTS_FILE), "[[[ broken").unwrap();
        assert!(list_vaults(&data).is_empty());

        let v = dir.path().join("v");
        fs::create_dir_all(&v).unwrap();
        let list = add_vault(&data, &v).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list_vaults(&data), list);
        assert_eq!(
            fs::read_to_string(data.join(format!("{VAULTS_FILE}.corrupt"))).unwrap(),
            "[[[ broken"
        );
        // Valid JSON of the wrong shape is tolerated as well.
        fs::write(data.join(VAULTS_FILE), "{\"vaults\": 3}").unwrap();
        assert!(list_vaults(&data).is_empty());
        fs::write(data.join(VAULTS_FILE), "{}").unwrap();
        assert!(list_vaults(&data).is_empty());
    }

    #[test]
    fn vault_for_path_matches_whole_components_and_deepest_vault() {
        let dir = TempDir::new("vaults-for");
        let data = dir.path().join("data");
        let outer = dir.path().join("vault");
        let inner = outer.join("projects/inner");
        let sibling = dir.path().join("vault2");
        fs::create_dir_all(&inner).unwrap();
        fs::create_dir_all(&sibling).unwrap();
        add_vault(&data, &outer).unwrap();
        add_vault(&data, &inner).unwrap();

        let name = |p: PathBuf| vault_for_path(&data, &p).map(|v| v.name);
        assert_eq!(name(outer.join("a/b.md")), Some("vault".into()));
        assert_eq!(name(outer.clone()), Some("vault".into()));
        assert_eq!(name(inner.join("x.md")), Some("inner".into()));
        // The file does not have to exist.
        assert_eq!(name(outer.join("not/yet/there.md")), Some("vault".into()));
        // Same prefix but another folder: not in the vault.
        assert_eq!(name(sibling.join("a.md")), None);
        assert_eq!(name(dir.path().join("loose.md")), None);
    }

    #[test]
    fn registry_json_shape() {
        let dir = TempDir::new("vaults-json");
        let v = dir.path().join("v");
        fs::create_dir_all(&v).unwrap();
        let list = add_vault(dir.path(), &v).unwrap();
        let json = serde_json::to_value(&list).unwrap();
        assert_eq!(json[0]["name"], "v");
        assert_eq!(json[0]["path"], v.to_str().unwrap());
    }
}
