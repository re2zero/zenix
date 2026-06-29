# Logging Guidelines

> Structured logging with the `tracing` crate throughout the project.

---

## Overview

The project uses **`tracing`** (not `log` or `println!`) for all logging. The `tracing` macro crate provides `tracing::info!`, `tracing::warn!`, `tracing::error!`, and `tracing::debug!`.

## Log Levels

### `tracing::info!` — Normal operations

Use for things that happen normally during operation:

```rust
tracing::info!("herdr server started: {binary:?}");
tracing::info!("spawning PTY: {program} {args:?}");
tracing::info!("socket ready, spawning PTY");
tracing::info!("installed skill: {name}");
tracing::info!("terminal title: {title}");
```

### `tracing::warn!` — Recoverable issues

Use when something unexpected happened but the app can continue:

```rust
tracing::warn!("failed to load app icon");
tracing::warn!("failed to load embedded theme: {err:#}");
tracing::warn!("pty closed: {reason}");
tracing::warn!("install failed: {e}");
tracing::warn!("fetch models failed for {name}: {e}");
```

**Format note**: Use `{err:#}` (Debug format) for error display — gives full backtrace context.

### `tracing::error!` — Failure to continue

Use when an operation genuinely failed and the user may need to know:

```rust
tracing::error!("failed to start herdr server: {err}");
tracing::error!("spawn_pty failed: {err}");
```

### `tracing::debug!` — Development only

Not currently used in production code. Reserved for verbose debugging.

## Structured Logging Format

The project uses **message-style logging** (not field-based structured logging). Each log line is a human-readable message with interpolation:

```rust
// Message-style (current convention)
tracing::info!("installed skill: {name}");
tracing::warn!("install failed for {name}: {e}");

// Not used yet but acceptable for complex structs:
tracing::info!("herdr server started: {binary:?}");
```

### Format specifiers used

| Specifier | When to use |
|-----------|-------------|
| `{var}` | Display format (default) |
| `{var:#}` | Debug format (errors, complex types) |
| `{var:?}` | Debug format (structs, paths) |

## What to Log

- Module/feature lifecycle events (start, stop, ready)
- User-triggered actions (install, remove, toggle)
- External calls (herdr IPC, API providers, PTY spawn)
- Configuration changes (load, save)
- State transitions (socket ready, PTY closed)

## What NOT to Log

- Sensitive data (API keys, tokens, config file contents)
- Raw terminal output (PTY byte streams)
- High-frequency UI render cycles
- Routine GPUI event processing

## Known Exception: `eprintln!` in Socket IPC

`eprintln!` is used in production for socket IPC errors (`src/single_instance.rs`, `src/main.rs:152`, `src/app.rs:443`). This is legacy code that predates the tracing convention. New code should use `tracing::error!` instead.

```rust
// Legacy (don't add new ones)
eprintln!("failed to acquire lock: {err}");

// New code — use tracing
tracing::error!("failed to acquire lock: {err}");
```

## Examples from Codebase

**Good — clear, contextual**:
```rust
tracing::info!("socket ready, spawning PTY");
tracing::warn!("pty closed: {reason}");
tracing::error!("spawn_pty failed: {err}");
```

**Bad — too vague**:
```rust
tracing::info!("done");
tracing::warn!("error");
```

## Common Mistakes

1. **Using `println!` or `eprintln!` for new code**: Always use `tracing::info!`/`warn!`/`error!`. Legacy `eprintln!` in socket IPC code is documented separately.
2. **Logging errors without context**: Always include what operation failed.
3. **Over-logging**: Don't log every GPUI event tick or render frame.
