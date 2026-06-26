//! Configuration file scanner — discover, read, modify, and write
//! JSON/JSONC config files while preserving unknown fields.
//!
//! `ConfigMap` is the core type. It loads a JSON or JSONC file,
//! provides path-based field access/upsert/removal, and saves
//! with automatic timestamped backup.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::jsonc;

/// A JSON/JSONC config file loaded into memory with field-preserving writes.
///
/// # Path-based access
///
/// All field operations use a slice of path segments:
/// - `get(&["mcpServers", "my-server", "command"])` navigates nested objects.
/// - `upsert(&["mcpServers", "my-server"], server_value)` creates intermediate objects.
/// - `remove(&["mcpServers", "old-server"])` removes a key, keeping siblings.
pub struct ConfigMap {
    /// Absolute path to the config file on disk.
    path: PathBuf,
    /// Parsed JSON root value.
    root: Value,
}

impl ConfigMap {
    /// Load a JSON or JSONC config file from disk.
    ///
    /// If the file doesn't exist, creates an empty object `{}` as the root.
    /// If the file exists but is empty/unparseable, returns an error.
    pub fn load(path: &Path) -> Result<Self, String> {
        let root = if path.exists() {
            jsonc::read_json_file(path)?
        } else {
            Value::Object(serde_json::Map::new())
        };
        Ok(Self {
            path: path.to_path_buf(),
            root,
        })
    }

    /// Get a reference to a value by path segments.
    ///
    /// Returns `None` if any segment in the path doesn't exist or isn't an object.
    pub fn get(&self, path: &[&str]) -> Option<&Value> {
        let mut current = &self.root;
        for seg in path {
            current = current.as_object()?.get(*seg)?;
        }
        Some(current)
    }

    /// Get a mutable reference to a value by path segments.
    pub fn get_mut(&mut self, path: &[&str]) -> Option<&mut Value> {
        let mut current = &mut self.root;
        for seg in path {
            current = current.as_object_mut()?.get_mut(*seg)?;
        }
        Some(current)
    }

    /// Upsert a value at the given path, creating intermediate objects as needed.
    ///
    /// If the path is empty, replaces the entire root.
    pub fn upsert(&mut self, path: &[&str], value: Value) {
        if path.is_empty() {
            self.root = value;
            return;
        }

        let mut current = &mut self.root;
        // Navigate to parent, creating objects along the way
        for (i, seg) in path.iter().enumerate() {
            let is_last = i == path.len() - 1;
            if is_last {
                if let Some(obj) = current.as_object_mut() {
                    obj.insert(seg.to_string(), value.clone());
                }
            } else {
                // Ensure next level is an object
                if !current.is_object() {
                    *current = Value::Object(serde_json::Map::new());
                }
                let obj = current.as_object_mut().unwrap();
                if !obj.contains_key(*seg) {
                    obj.insert(seg.to_string(), Value::Object(serde_json::Map::new()));
                }
                // Reborrow: navigate into the object
                current = obj.get_mut(*seg).unwrap();
            }
        }
    }

    /// Remove a value at the given path.
    ///
    /// Returns the removed value, or `None` if the path doesn't exist.
    pub fn remove(&mut self, path: &[&str]) -> Option<Value> {
        if path.is_empty() {
            let old = std::mem::replace(&mut self.root, Value::Object(serde_json::Map::new()));
            return Some(old);
        }
        let parent_path = &path[..path.len() - 1];
        let last_seg = path[path.len() - 1];
        let parent = self.get_mut(parent_path)?;
        parent.as_object_mut()?.remove(last_seg)
    }

    /// Save the config back to disk.
    ///
    /// Creates a timestamped backup before overwriting the original file.
    /// Creates parent directories if they don't exist.
    pub fn save(&self) -> Result<(), String> {
        // Create parent directory
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create dir {}: {e}", parent.display()))?;
        }

        // Backup with timestamp
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let bak_path = format!(
            "{}.zenix.{}.bak",
            self.path.file_name().unwrap_or_default().to_string_lossy(),
            timestamp
        );
        let bak_full = self.path.with_file_name(&bak_path);
        if self.path.exists() {
            let _ = std::fs::copy(&self.path, &bak_full);
        }

        // Serialize and write
        let pretty = serde_json::to_string_pretty(&self.root)
            .map_err(|e| format!("serialize error: {e}"))?;
        std::fs::write(&self.path, &pretty)
            .map_err(|e| format!("write {}: {e}", self.path.display()))?;

        Ok(())
    }

    /// Return a reference to the underlying path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return a reference to the root JSON value.
    pub fn root(&self) -> &Value {
        &self.root
    }

    /// Consume self and return the root JSON value.
    pub fn into_root(self) -> Value {
        self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("zenix_test_scanner");
        let _ = std::fs::create_dir_all(&dir);
        dir.join(name)
    }

    #[test]
    fn test_load_nonexistent() {
        let path = temp_path("nonexistent.json");
        let _ = std::fs::remove_file(&path);
        let map = ConfigMap::load(&path).unwrap();
        assert!(map.root().is_object());
    }

    #[test]
    fn test_load_and_get() {
        let path = temp_path("test_get.json");
        std::fs::write(&path, r#"{"mcpServers": {"srv": {"command": "npx"}}}"#).unwrap();

        let map = ConfigMap::load(&path).unwrap();
        let cmd = map.get(&["mcpServers", "srv", "command"]).unwrap();
        assert_eq!(cmd.as_str().unwrap(), "npx");
    }

    #[test]
    fn test_upsert_new_path() {
        let path = temp_path("test_upsert.json");
        std::fs::write(&path, r#"{"existing": true}"#).unwrap();

        let mut map = ConfigMap::load(&path).unwrap();
        map.upsert(&["mcpServers", "new_srv", "command"], json!("echo"));

        let cmd = map.get(&["mcpServers", "new_srv", "command"]).unwrap();
        assert_eq!(cmd.as_str().unwrap(), "echo");
        // Existing field preserved
        assert_eq!(map.get(&["existing"]).unwrap().as_bool().unwrap(), true);
    }

    #[test]
    fn test_upsert_overwrite() {
        let path = temp_path("test_overwrite.json");
        std::fs::write(&path, r#"{"key": "old"}"#).unwrap();

        let mut map = ConfigMap::load(&path).unwrap();
        map.upsert(&["key"], json!("new"));

        assert_eq!(map.get(&["key"]).unwrap().as_str().unwrap(), "new");
    }

    #[test]
    fn test_remove() {
        let path = temp_path("test_remove.json");
        std::fs::write(&path, r#"{"a": 1, "b": 2}"#).unwrap();

        let mut map = ConfigMap::load(&path).unwrap();
        let removed = map.remove(&["a"]).unwrap();
        assert_eq!(removed.as_i64().unwrap(), 1);
        assert!(map.get(&["a"]).is_none());
        assert_eq!(map.get(&["b"]).unwrap().as_i64().unwrap(), 2);
    }

    #[test]
    fn test_save_and_reload() {
        let path = temp_path("test_save.json");
        std::fs::write(&path, r#"{"keep": "me"}"#).unwrap();

        let mut map = ConfigMap::load(&path).unwrap();
        assert_eq!(map.get(&["keep"]).unwrap().as_str().unwrap(), "me");
        map.upsert(&["added"], json!(42));
        assert_eq!(map.get(&["added"]).unwrap().as_i64().unwrap(), 42);
        map.save().unwrap();

        // Verify file was written correctly
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("added"), "saved file missing 'added': {}", raw);
        assert!(raw.contains("keep"), "saved file missing 'keep': {}", raw);

        let reloaded = ConfigMap::load(&path).unwrap();
        assert_eq!(reloaded.get(&["keep"]).unwrap().as_str().unwrap(), "me");
        assert_eq!(reloaded.get(&["added"]).unwrap().as_i64().unwrap(), 42);
    }

    #[test]
    fn test_load_jsonc() {
        let path = temp_path("test_jsonc.json");
        std::fs::write(
            &path,
            "{\n  // MCP config\n  \"mcpServers\": { /* inline */ \"srv\": {\"command\": \"npx\"} }\n}",
        )
        .unwrap();

        let map = ConfigMap::load(&path).unwrap();
        let cmd = map.get(&["mcpServers", "srv", "command"]).unwrap();
        assert_eq!(cmd.as_str().unwrap(), "npx");
    }

    #[test]
    fn test_nested_upsert_creates_intermediate() {
        let path = temp_path("test_nested.json");
        std::fs::write(&path, "{}").unwrap();

        let mut map = ConfigMap::load(&path).unwrap();
        map.upsert(&["a", "b", "c", "d"], json!("deep"));

        let val = map.get(&["a", "b", "c", "d"]).unwrap();
        assert_eq!(val.as_str().unwrap(), "deep");
    }
}
