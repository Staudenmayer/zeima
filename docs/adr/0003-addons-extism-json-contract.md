# ADR 0003: Addons as Extism WASM plugins with a JSON contract

- Status: accepted
- Date: 2026-10-10

## Context

Integrations (calendars, issue trackers, custom export formats) should not be
built into the app (SPEC §5.12, PLAN Phase 4). Third parties must be able to
write them, and users must be able to install them without trusting them with
the whole machine or all their data.

## Decision

- An addon is a WASM module plus `addon.toml`. The app runs it with Extism
  (wasmtime) without WASI: no file system, clock or sockets.
- Access is explicit: the manifest declares permissions, network hosts and
  hooks; the user sees them before installing and grants all or nothing.
  Every host function checks its permission on each call. HTTP uses Extism's
  client, limited to `allowed_hosts`.
- Each call has a time limit (10 s, 0.8 s for quick-switch suggestions), each
  plugin a memory cap (64 MB). Failures are recorded and shown, never fatal.
- Addon writes go through `time-core` like UI writes (validated,
  change-logged) and are recorded with source `addon` and the addon's id.
- The contract is JSON with camelCase fields, defined once in
  `crates/time-addon-sdk` and used by both the host and addons. Fields are
  only ever added within a major version; unknown fields are ignored.
- Domain events come from a bus in `time-core`: repositories queue them on
  the write transaction and they are published after commit.
- Installed addons, their settings and key-value storage are device-local
  tables, not synced: the modules live on disk on one machine.

## Consequences

- Addons can be written in any language with an Extism PDK; Rust gets typed
  helpers from the SDK.
- wasmtime adds about 1.5 minutes to a clean build and several MB to the app.
- Settings and secrets of addons stay on the device; syncing them (and a
  signed addon registry) are later decisions.
- Changing a payload incompatibly needs a new major SDK version.
