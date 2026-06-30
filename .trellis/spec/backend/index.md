# Backend / Core Development Guidelines

> Guidelines for the Rust + GPUI core of zenix.

---

## Overview

Zenix is a cross-platform GPUI desktop frontend. There is no traditional "backend" tier — the entire app lives in `src/`. This directory covers code organization, error handling, logging, and quality standards for the core Rust codebase.

---

## Guidelines Index

| Guide | Description | Status |
|-------|-------------|--------|
| [Directory Structure](./directory-structure.md) | Module layout and file conventions | ✅ Populated |
| [Error Handling](./error-handling.md) | anyhow::Context, Result types, no unwrap | ✅ Populated |
| [Logging Guidelines](./logging-guidelines.md) | tracing crate, structured logs | ✅ Populated |
| [Quality Guidelines](./quality-guidelines.md) | GPUI patterns, clippy, formatting | ✅ Populated |
| [AI Orchestrator](./ai-orchestrator.md) | Multi-agent orchestration: LLM + herdr IPC + dispatch loop | ✅ Populated |

> **Note**: This project has no database layer. `database-guidelines.md` is intentionally omitted — if a database is added in the future, create that file then.

---

## Key Conventions at a Glance

- **Rust 2024 edition**, GPL-3.0-or-later
- **GPUI Entity/Context/Action** pattern throughout
- **No `unwrap()`** — use `anyhow::Context` / `thiserror`
- **Platform code** gated in `src/platform.rs` only
- **Config** = JSONC parsed via `toml` crate
- **Logging** via `tracing` crate (info/warn/error)
- **I18n**: Chinese/English via `rust-i18n`
