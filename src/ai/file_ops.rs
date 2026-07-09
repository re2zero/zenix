//! Internal file operation utilities extracted from the tool system.
//!
//! These functions provide file read, directory listing, and glob-based
//! file search capabilities. They are NOT registered as orchestrator tools
//! but are available for internal use (e.g., context collection).

/// Maximum file read size (50 KB).
#[allow(dead_code)]
pub const MAX_FILE_READ_BYTES: usize = 50 * 1024;

/// Maximum directory listing depth.
#[allow(dead_code)]
pub const MAX_DIR_DEPTH: usize = 3;

/// Default maximum search results.
#[allow(dead_code)]
pub const DEFAULT_MAX_SEARCH_RESULTS: u64 = 50;

/// Read a file's contents up to `MAX_FILE_READ_BYTES`.
/// Returns `Ok(content)` on success, `Err(message)` on failure.
#[allow(dead_code)]
pub fn read_file_contents(path: &str) -> Result<String, String> {
    let path = std::path::Path::new(path);
    if !path.exists() {
        return Err(format!("文件不存在: {}", path.display()));
    }
    if !path.is_file() {
        return Err(format!("路径不是文件: {}", path.display()));
    }

    match std::fs::read(path) {
        Ok(bytes) => {
            if bytes.len() > MAX_FILE_READ_BYTES {
                let truncated = String::from_utf8_lossy(&bytes[..MAX_FILE_READ_BYTES]);
                let remaining = bytes.len() - MAX_FILE_READ_BYTES;
                Ok(format!(
                    "{}\n\n[TRUNCATED: 内容已截断，剩余 {} 字节未显示]",
                    truncated, remaining
                ))
            } else {
                String::from_utf8(bytes).map_err(|e| format!("文件包含无效的 UTF-8 数据: {e}"))
            }
        }
        Err(e) => Err(format!("读取文件失败: {e}")),
    }
}

/// Recursively list directory contents (like `tree`) up to `MAX_DIR_DEPTH` levels.
/// Returns `Ok(output)` on success, `Err(message)` on failure.
#[allow(dead_code)]
pub fn list_directory(path: &str) -> Result<String, String> {
    let path = std::path::Path::new(path);
    if !path.exists() {
        return Err(format!("目录不存在: {}", path.display()));
    }
    if !path.is_dir() {
        return Err(format!("路径不是目录: {}", path.display()));
    }

    let mut output = String::new();
    list_dir_recursive(path, &mut output, "", 0, MAX_DIR_DEPTH);

    if output.is_empty() {
        return Ok("(空目录)".to_string());
    }

    Ok(output.trim_end_matches('\n').to_string())
}

/// Recursively list directory contents.
fn list_dir_recursive(
    dir: &std::path::Path,
    output: &mut String,
    prefix: &str,
    depth: usize,
    max_depth: usize,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            output.push_str(&format!("{prefix}⚠ 无法读取: {e}\n"));
            return;
        }
    };

    let mut dirs: Vec<_> = Vec::new();
    let mut files: Vec<_> = Vec::new();

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        match entry.file_type() {
            Ok(ft) if ft.is_dir() => dirs.push((name, entry.path())),
            _ => files.push((name, entry.path())),
        }
    }

    dirs.sort_by(|a, b| a.0.cmp(&b.0));
    files.sort_by(|a, b| a.0.cmp(&b.0));

    for (i, (name, child_path)) in dirs.iter().enumerate() {
        let is_last = i == dirs.len() - 1 && files.is_empty();
        let connector = if is_last { "└─" } else { "├─" };
        output.push_str(&format!("{prefix}{connector} {name}/\n"));

        if depth + 1 < max_depth {
            let child_prefix = format!("{prefix}{}  ", if is_last { "  " } else { "│ " });
            list_dir_recursive(child_path, output, &child_prefix, depth + 1, max_depth);
        } else {
            let child_prefix = format!("{prefix}{}  ", if is_last { "  " } else { "│ " });
            if let Ok(child_entries) = std::fs::read_dir(child_path) {
                let mut children: Vec<_> = child_entries
                    .flatten()
                    .map(|e| (e.file_name().to_string_lossy().to_string(), e.path()))
                    .filter(|(n, _)| !n.starts_with('.'))
                    .collect();
                children.sort_by(|a, b| a.0.cmp(&b.0));
                for (j, (cname, cpath)) in children.iter().enumerate() {
                    let c_last = j == children.len() - 1;
                    let c_connector = if c_last { "└─" } else { "├─" };
                    let is_dir = std::fs::symlink_metadata(cpath)
                        .ok()
                        .map(|m| m.file_type().is_dir())
                        .unwrap_or(false);
                    if is_dir {
                        output.push_str(&format!("{child_prefix}{c_connector} {cname}/\n"));
                    } else {
                        output.push_str(&format!("{child_prefix}{c_connector} {cname}\n"));
                    }
                }
            }
        }
    }

    for (i, (name, full_path)) in files.iter().enumerate() {
        let is_last = i == files.len() - 1;
        let connector = if is_last { "└─" } else { "├─" };
        let is_dir = std::fs::symlink_metadata(full_path)
            .ok()
            .map(|m| m.file_type().is_dir())
            .unwrap_or(false);
        if is_dir {
            output.push_str(&format!("{prefix}{connector} {name}/\n"));
        } else {
            if let Ok(meta) = std::fs::metadata(full_path) {
                let size = meta.len();
                output.push_str(&format!("{prefix}{connector} {name} ({size} bytes)\n"));
            } else {
                output.push_str(&format!("{prefix}{connector} {name}\n"));
            }
        }
    }
}

/// Recursively search for files matching a glob pattern.
#[allow(dead_code)]
pub fn search_files(path: &str, pattern: &str, max_results: u64) -> Result<String, String> {
    let search_path = std::path::Path::new(path);
    if !search_path.exists() {
        return Err(format!("搜索路径不存在: {}", path));
    }
    if !search_path.is_dir() {
        return Err(format!("路径不是目录: {}", path));
    }

    let mut results: Vec<String> = Vec::new();
    search_files_recursive(
        search_path,
        pattern,
        &mut results,
        &mut 0,
        max_results as usize,
    );

    if results.is_empty() {
        Ok(format!("未找到匹配 \"{pattern}\" 的文件"))
    } else {
        Ok(format!(
            "找到 {} 个匹配文件:\n\n{}",
            results.len(),
            results.join("\n")
        ))
    }
}

/// Recursively search for files matching a glob pattern.
fn search_files_recursive(
    dir: &std::path::Path,
    pattern: &str,
    results: &mut Vec<String>,
    count: &mut usize,
    max_results: usize,
) {
    if *count >= max_results {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        if *count >= max_results {
            return;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }

        let full_path = entry.path();
        let file_name = full_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        if matches_glob(&file_name, pattern) {
            results.push(full_path.to_string_lossy().to_string());
            *count += 1;
        }

        if let Ok(ft) = entry.file_type() {
            if ft.is_dir() && (pattern == "**" || pattern == "**/*") {
                results.push(full_path.to_string_lossy().to_string());
                *count += 1;
            }
            if ft.is_dir() {
                search_files_recursive(&full_path, pattern, results, count, max_results);
            }
        }
    }
}

/// Simple glob matching supporting `*` (any chars in one segment) and `**` (any path).
fn matches_glob(file_name: &str, pattern: &str) -> bool {
    if pattern.contains("**") {
        let parts: Vec<&str> = pattern.split("**").collect();
        if parts.len() == 2 {
            let prefix = parts[0].trim_end_matches('/');
            let suffix = parts[1].trim_start_matches('/');

            if prefix.is_empty() && suffix.is_empty() {
                return true;
            }

            if prefix.is_empty() {
                if suffix.is_empty() {
                    return true;
                }
                return matches_glob(file_name, suffix);
            }

            if suffix.is_empty() {
                return file_name.starts_with(prefix);
            }

            return file_name.starts_with(prefix)
                && matches_glob(file_name.strip_prefix(prefix).unwrap_or(""), suffix);
        }
    }

    glob_match(file_name, pattern)
}

/// Match a single-segment glob pattern against a file name.
fn glob_match(s: &str, pattern: &str) -> bool {
    let s_bytes = s.as_bytes();
    let pat_bytes = pattern.as_bytes();

    let sn = s_bytes.len();
    let pn = pat_bytes.len();
    let mut dp = vec![vec![false; pn + 1]; sn + 1];
    dp[0][0] = true;

    for j in 1..=pn {
        if pat_bytes[j - 1] == b'*' {
            dp[0][j] = dp[0][j - 1];
        }
    }

    for i in 1..=sn {
        for j in 1..=pn {
            if pat_bytes[j - 1] == b'*' {
                dp[i][j] = dp[i][j - 1] || dp[i - 1][j];
            } else if pat_bytes[j - 1] == b'?' || s_bytes[i - 1] == pat_bytes[j - 1] {
                dp[i][j] = dp[i - 1][j - 1];
            }
        }
    }

    dp[sn][pn]
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_file_exists() {
        let dir = std::env::temp_dir().join("zenix_fops_test");
        let _ = std::fs::create_dir_all(&dir);
        let file_path = dir.join("test_read.txt");
        std::fs::write(&file_path, "Hello, World!").unwrap();

        let result = read_file_contents(file_path.to_str().unwrap());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Hello, World!");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_file_not_exists() {
        let result = read_file_contents("/nonexistent/file.txt");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("不存在"));
    }

    #[test]
    fn test_read_file_truncated() {
        let dir = std::env::temp_dir().join("zenix_fops_trunc");
        let _ = std::fs::create_dir_all(&dir);
        let file_path = dir.join("big_file.txt");
        let content = "x".repeat(MAX_FILE_READ_BYTES + 1000);
        std::fs::write(&file_path, &content).unwrap();

        let result = read_file_contents(file_path.to_str().unwrap());
        assert!(result.is_ok());
        assert!(result.as_ref().unwrap().contains("[TRUNCATED"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_list_directory() {
        let dir = std::env::temp_dir().join("zenix_fops_dir");
        let _ = std::fs::create_dir_all(&dir);
        let sub = dir.join("subdir");
        let _ = std::fs::create_dir_all(&sub);
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        std::fs::write(dir.join("b.rs"), "b").unwrap();
        std::fs::write(sub.join("c.toml"), "c").unwrap();

        let result = list_directory(dir.to_str().unwrap());
        assert!(result.is_ok());
        assert!(result.as_ref().unwrap().contains("a.txt"));
        assert!(result.as_ref().unwrap().contains("b.rs"));
        assert!(result.as_ref().unwrap().contains("subdir"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_list_directory_not_exists() {
        let result = list_directory("/nonexistent/dir");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("不存在"));
    }

    #[test]
    fn test_search_files_glob() {
        let dir = std::env::temp_dir().join("zenix_fops_search");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("main.rs"), "").unwrap();
        std::fs::write(dir.join("lib.rs"), "").unwrap();
        std::fs::write(dir.join("config.toml"), "").unwrap();

        let result = search_files(dir.to_str().unwrap(), "*.rs", 50);
        assert!(result.is_ok());
        let content = result.unwrap();
        assert!(content.contains("main.rs"));
        assert!(content.contains("lib.rs"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_search_files_max_results() {
        let dir = std::env::temp_dir().join("zenix_fops_limit");
        let _ = std::fs::create_dir_all(&dir);
        for i in 0..10 {
            std::fs::write(dir.join(format!("file_{i}.rs")), "").unwrap();
        }

        let result = search_files(dir.to_str().unwrap(), "*.rs", 3);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().matches("file_").count(), 3);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
