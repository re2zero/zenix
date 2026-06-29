# Error Handling

> How errors are caught, wrapped, and returned in this project.

---

## Overview

This project uses **`anyhow`** for error handling throughout. There are **no custom error types** via `thiserror`. Note: `thiserror = "1"` is in Cargo.toml but unused — consider removing it in a future cleanup. Errors are wrapped with `.context("description")?` and returned as `anyhow::Result`.

## Error Patterns

### Pattern 1: anyhow::Context with `?` (primary pattern)

Used for I/O, parsing, and external calls where context matters for debugging.

```rust
use anyhow::{Context, Result};

// Good — adds context to the error chain
let content = fs::read_to_string(&path).context("read config file")?;
let child = pair.slave.spawn_command(cmd).context("spawn command")?;

// Good — shorter form via trait import
use anyhow::Context as _;
let reader = master.try_clone_reader().context("clone reader")?;
```

### Pattern 2: anyhow::Result as return type

```rust
pub fn load() -> Result<Self> { ... }
pub fn save(&self) -> Result<()> { ... }
fn start_pty(...) -> anyhow::Result<BackendTx> { ... }
```

### Pattern 3: String errors for simple cases

Some utility functions return `Result<T, String>` when the error doesn't need a chain:

```rust
pub fn symlink_dir(src: &Path, dst: &Path) -> Result<(), String> { ... }
pub fn parse_jsonc(raw: &str) -> Result<serde_json::Value, String> { ... }
pub fn test_connection(config: &ApiProviderConfig) -> Result<String, String> { ... }
```

### Pattern 4: `let _ =` for fire-and-forget (acceptable in UI code)

When an error is truly non-critical (e.g., icon loading, theme loading), the result is discarded:

```rust
// Acceptable — non-critical, error logged separately
let _ = tx.send(command);
```

## `unwrap()` Rule

**Prefer not to use `.unwrap()` in production paths.** Use `anyhow::Context` / `?` operator instead. There are a few legacy violations (e.g., `src/ui/settings.rs:1435`) that should be fixed in a future cleanup.

```rust
// ❌ Bad
let config = ConfigStore::load().unwrap();

// ✅ Good
let config = ConfigStore::load().context("load config")?;
```

## Error Reporting in UI

For user-facing errors (settings save, provider add, etc.), errors are logged via `tracing::warn!` or `tracing::error!` and displayed in the UI. Pattern from `src/app.rs`:

```rust
Ok(name) => tracing::info!("installed skill: {name}"),
Err(e) => tracing::warn!("install failed: {e}"),
```

## Validation & Error Matrix

| Situation | Approach |
|-----------|----------|
| I/O operation (file, network) | `.context("action")?` + `anyhow::Result` |
| Parse/decode | `.context("action")?` + `anyhow::Result` |
| Simple utility function | `Result<T, String>` |
| Non-critical failure (icon, theme) | Log + discard, don't propagate |
| External API call | `.context("action")?` + `anyhow::Result` |
| Internal invariant violation | `panic!()` or `unreachable!()` (rare) |

## Good / Bad Cases

**Good** — contextual error chain:
```rust
let content = fs::read_to_string(&path)
    .context("read config file")?;
let config: ConfigStore = serde_json::from_str(&content)
    .context("parse config")?;
```

**Bad** — lost context:
```rust
let content = fs::read_to_string(&path)?; // What failed?
```

**Bad** — unwrap:
```rust
let config = ConfigStore::load().unwrap(); // Runtime crash risk
```

## Common Mistakes

1. **Forgetting to import `Context`**: `.context()` is a trait method — must be in scope (`use anyhow::Context;` or `use anyhow::Context as _;`).
2. **Over-using String errors**: If the error chain matters for debugging, use `anyhow::Result` not `String`.
3. **unwrap() in `render()`**: Never — use action handlers or callbacks for state changes.
