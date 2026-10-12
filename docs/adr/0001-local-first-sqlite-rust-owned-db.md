# ADR 0001: Local-first storage in SQLite, owned by Rust

- Status: accepted
- Date: 2026-10-04

## Context

Zeima must work fully offline for one person, keep tracking while the window is
hidden, and later sync to a server for paid tiers (see SPEC §3, §6). The
scaffold used `tauri-plugin-sql` with the Postgres feature, which lets the
webview send raw SQL.

## Decision

- Data lives in one SQLite file (`zeima.db`, WAL mode) in the app data folder.
- Only Rust touches the database: `crates/zeima-core` (sqlx) behind typed Tauri
  commands generated with tauri-specta (`src/bindings.ts`). The webview never
  sends SQL. `tauri-plugin-sql` is removed.
- Every write goes through `Core::begin()` → `WriteTx`, which stamps rows with
  an HLC and appends to `change_log` in the same transaction.
- Synced tables carry the common columns (UUIDv7 `id`, `workspace_id`,
  timestamps, `deleted_at` tombstone, `hlc`, `origin_device`) from their first
  migration.
- Pure domain logic in `zeima-core` must not depend on sqlx/tokio (those sit
  behind the `db` feature) so it can be compiled to WASM for the server.

## Consequences

- One write path: validation, change logging and (later) permissions live in
  one place, and the timer works without the UI.
- Adding a command means touching Rust and regenerating bindings
  (`cargo test` does this; CI fails on drift).
- Deletes are soft deletes; queries must filter `deleted_at IS NULL`.
