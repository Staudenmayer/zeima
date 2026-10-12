# Time

Local-first time tracking and work-time management that lives in the system
tray. Tauri 2 + Vue 3, with a Rust core on SQLite.

- Product and technical spec: [docs/SPEC.md](docs/SPEC.md)
- Implementation plan: [docs/PLAN.md](docs/PLAN.md)
- Decisions: [docs/adr/](docs/adr/)

## Layout

| Path | What |
|---|---|
| `crates/time-core` | Domain models, validation, SQLite repositories, timer engine. Pure logic builds without the `db` feature. |
| `src-tauri` | Tauri shell: tray, global shortcuts, windows, IPC commands, background heartbeat |
| `src` | Vue frontend. `src/bindings.ts` is generated from Rust; do not edit. |

## Development

Requirements: Rust (stable), Bun, and the [Tauri system dependencies](https://tauri.app/start/prerequisites/).

```sh
bun install
bun run tauri dev        # run the app
cargo test --workspace --all-features   # Rust tests; also regenerates src/bindings.ts
bun run test             # frontend unit tests
bun run typecheck
```

Data is stored in `time.db` in the app data folder (shown under Settings → About).

## Shortcuts (defaults, configurable)

| Shortcut | Action |
|---|---|
| `Ctrl+Shift+T` | Show / hide the window |
| `Ctrl+Shift+Space` | Quick switch: `#ALIAS @tag note` |
| `Ctrl+Shift+P` | Pause / resume |
