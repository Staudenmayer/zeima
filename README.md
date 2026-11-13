# Zeima

Local-first time tracking and work-time management that lives in the system
tray. Tauri 2 + Vue 3, with a Rust core on SQLite.

- Product and technical spec: [docs/SPEC.md](docs/SPEC.md)
- Implementation plan: [docs/PLAN.md](docs/PLAN.md)
- Decisions: [docs/adr/](docs/adr/)

## Layout

| Path | What |
|---|---|
| `crates/zeima-core` | Domain models, validation, SQLite repositories, timer engine. Pure logic builds without the `db` feature. |
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

### Dev container

`.devcontainer/` sets up Rust, Node 24, Bun and the Tauri libraries, and starts Postgres, Keycloak (with the `zeima` realm) and Mailpit next to it. Open the folder in VS Code and choose "Reopen in Container". Inside, `cargo test --workspace --all-features`, `bun run test`, `bun run --cwd server test` and `bun run --cwd server dev` work as above. The desktop window (`tauri dev`) needs a display, so run that on the host; the container is for the Rust core, the frontend checks and the server.

Data is stored in `zeima.db` in the app data folder (shown under Settings → About).

## Shortcuts (defaults, configurable)

| Shortcut | Action |
|---|---|
| `Ctrl+Shift+T` | Show / hide the window |
| `Ctrl+Shift+Space` | Quick switch: `#ALIAS @tag note` |
| `Ctrl+Shift+P` | Pause / resume |
