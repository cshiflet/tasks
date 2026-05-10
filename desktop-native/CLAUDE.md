# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

This file scopes the **`desktop-native/`** sub-project (Rust + Qt 6 native client). The repo-root `CLAUDE.md` covers the multi-module Android/KMP/Wear layout and the *do-not-read* `jetpack-desktop` branch — read both.

## Build & test loop

```sh
# Type-check only (3-10× faster than build; fastest inner loop)
cargo check
# Pure-Rust crate, no Qt rebuild — use this when iterating on tasks-core
cargo test -p tasks-core
# Workspace tests (recompiles cxx-qt; slower)
cargo test --workspace --no-fail-fast
# Single test
cargo test --workspace today_window_tests::utc_offset_returns_utc_midnight
# GUI smoke (headless)
QT_QPA_PLATFORM=offscreen cargo run -p tasks-ui
# Pre-merge gate (mirrors CI)
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
# Dep vulnerabilities (note: yanked-package check fails with 403 in this sandbox; real findings still surface)
cargo audit
```

`BUILD.md` covers Docker / Ubuntu / Fedora / Arch / macOS / Windows setup, the mold linker swap, and Qt-install troubleshooting.

## Branch + push constraints (enforced server-side by the sandbox proxy)

- Pushes are allowed **only** to refs under `refs/heads/claude/*`. Anything else → HTTP 403.
- **Tag pushes are denied.** If a release workflow needs a `desktop-native-v*` tag, hand the user the exact `git tag … && git push origin …` command — they run it locally.
- **Ref deletes are denied.** A branch you create on origin can't be undone from this side; pick names carefully.

## Architecture (the picture that needs multiple files to see)

Three crates in a Cargo workspace, with a deliberate one-way dependency graph: `tasks-ui → tasks-sync → tasks-core`. Anything Qt-flavoured stays in `tasks-ui`; anything network-flavoured stays in `tasks-sync`; `tasks-core` is pure Rust + `rusqlite`.

### `tasks-core` — schema-pinned data layer
- `db/mod.rs` opens the SQLite the Android app writes. **Identity-hash check** against `PINNED_SCHEMA_VERSION` / `PINNED_IDENTITY_HASH` rejects mismatched schemas at open time. Symlinks are rejected (L-5).
- `tune_writeback_connection(&Connection)` is the **canonical RW tuning call site** — sets `journal_mode=WAL` + `busy_timeout=5s`. Every RW handle in the workspace must go through it; without this, parallel-account sync deadlocks with `SQLITE_BUSY`.
- `query/{recursive,non_recursive,sort}.rs` is the port of the upstream `kmp/.../TaskListQuery*.kt`. SQL is built via `format!` interpolating **enum-derived constants only** (no user input reaches a `format!`+SQL site).
- `now_ms()` is the single time source (do not duplicate; the per-provider `now_ms` defs were consolidated).

### `tasks-sync` — provider-agnostic sync engine
- `provider.rs` defines the `Provider` trait every backend implements (`connect`, `list_calendars`, `list_tasks`, `push_task`, `delete_task(... etag)`, `create_calendar`).
- `engine.rs::SyncEngine` orchestrates `pull_all` → `push_dirty` (deletes BEFORE pushes; per-row tolerant — one failed row doesn't abort the cycle).
- `providers/{caldav,etesync,google,microsoft}.rs` — four impls. The CalDAV provider is the most invariant-heavy:
  - **`trusted_origin` is anchored to the user-supplied `server_url`** (NOT the server-returned `calendar_home`). Every authenticated request calls `trusted_origin.check(&url)` BEFORE the auth header travels — H-1 protection. Server-returned `<d:href>` values that resolve cross-origin must error, not silently follow.
  - **`If-Match` on PUT and DELETE.** 412 maps to `SyncError::Conflict { … }`; the engine counts it into `outcome.conflicts` and leaves `cd_etag` / `cd_deleted` unchanged so the next pull refreshes the etag.
- `loopback.rs::LoopbackReceiver` — OAuth callback receiver. **Host allowlist on `bind_with_redirect`** (`127.0.0.1` / `localhost` / `::1` / `[::1]`); peer rejected if not `is_loopback()`; per-stream slow-loris drop; single-use; Host header pinned. The 25 ms accept poll cadence is why CI deadlines on the race-shape tests are 5 s, not 2 s.
- `oauth.rs` — PKCE + state token generation via `getrandom`; `parse_redirect` rejects CR/LF in keys/values (header-injection close).
- `token_store.rs` defines the `TokenStore` trait; concrete impls live in `tasks-ui/src/token_persist.rs` (see below).

### `tasks-ui` — cxx-qt bridge + QML
- `src/bridge.rs` (~5K LOC) is the `TaskListViewModel` Q_INVOKABLE / Q_PROPERTY surface. **Keep this boundary thin** — viewmodels expose state and signals; UI logic stays in QML. Adding a new bridge field is expensive (cxx-qt regenerates bindings every time the bridge module changes).
- `qml/{Main,SidebarPane,TaskListPane,TaskDetailPane,AccountsPane,...}.qml` — three-pane layout, Material 2 system theme.
- `src/token_persist.rs` is the credential-storage probe cascade: **keyring → encrypted file → in-memory**, in that order. The `EncryptedFileTokenStore` uses AES-256-GCM with HKDF-SHA256 from `/etc/machine-id` (Linux) / IORegistry UUID (macOS) / SchedulerSupplied UUID (Windows); per-message random nonce; auth tag verified before any plaintext is returned.
- `src/notifier.rs` — alarm scheduler that drives `notify-rust` (Linux/macOS/Windows OS-native).
- `src/win_dark_titlebar.rs` is the only place outside cxx-qt-generated code where `unsafe extern "system"` lives — Win32 FFI for the dark-mode titlebar workaround.

## Invariants the sub-agents and reviewers expect to hold

These cross-cut multiple files; preserve them when editing.

1. **Schema pin** — any change under `data/schemas/` must bump `PINNED_SCHEMA_VERSION` + `PINNED_IDENTITY_HASH` together; `tests/schema_guard.rs` enforces this in CI.
2. **WAL + 5 s busy_timeout** — every `Connection` opened RW must call `tasks_core::tune_writeback_connection`. Search for `OpenFlags::SQLITE_OPEN_READ_WRITE` to find handles; each one needs the call.
3. **Trust-origin clamp** — when adding a CalDAV code path that issues an authenticated request, route the URL through `trusted_origin.check(&url)?` first. The origin is anchored on `connect()` to the user-supplied `server_url`, not to anything the server returns.
4. **`SecretString` discipline** — credentials are wrapped in `secrecy::SecretString` from the bridge boundary down. `expose_secret()` is called **only** at the FFI/HTTP-header construction site; never log the value.
5. **Error body truncation** — when embedding an HTTP response body into a `SyncError` the bridge will display, route it through `http_util::truncate_for_status` (256-char cap + CR/LF/TAB → space). Full body still goes to `tracing::debug`.
6. **`re-entrancy` on `sync_account`** — the `syncs_in_flight: HashSet<String>` insert is the load-bearing guard; insert just before `thread::Builder::spawn` AND remove the slot if spawn returns `Err` (don't `.expect()`).
7. **Tier-name compare in `update_credential_storage_choice`** — `Arc::ptr_eq` is wrong (probe always mints fresh `Arc::new`); compare `current_tier_str != new_tier_name` instead, otherwise the migration loop runs against the same store and destroys credentials it just rewrote.

## Test fixtures + how the UI is exercised

- Engine tests use `fresh_db()` + `seed_*_task()` helpers in `engine.rs::tests` to spin up a temp SQLite with the pinned schema, then drive a `MockWithPushResult` provider.
- Provider impls have unit tests for the synchronous helpers (XML parsing, header construction, color hex parse) but no live-server tests (CI has no credentials). End-to-end smoke against Radicale / Etebase / Fastmail is manual.
- The QML graph is exercised in CI via `QT_QPA_PLATFORM=offscreen cargo run -p tasks-ui` for ~3 s; stderr is grepped for `Binding loop` / `Unable to assign [undefined]` / `TypeError` / `: error:` patterns. Adding a QML binding loop fails CI.

## CI matrix gotchas

- **Linux** (`ubuntu-latest`) — fmt + clippy + tests + offscreen smoke. `libdbus-1-dev` + `mold` + `clang` apt-installed for the secret-service backend and the linker swap.
- **macOS** (`macos-latest`) — tests + offscreen smoke. The runner is slow; loopback race-shape tests use 5 s deadlines (not 2 s).
- **Windows** — pinned to `windows-2022` (MSVC 2019), not `windows-latest`. The cached Qt 6.6.3 is built against `win64_msvc2019_64`; `windows-2025` / VS 2026 image migration breaks the cache.
- **Schema-hash guard** runs only in `check-linux` since it's deterministic.

## Where else to read for context

- `BUILD.md` — toolchain matrix, dev loop, runtime env vars (`QT_QPA_PLATFORM`, `RUST_LOG`, `QT_LOGGING_RULES`), troubleshooting.
- `DECISIONS.md` — non-obvious technical choices (Rust over Go/C++, cxx-qt over Slint, schema pin, trust-origin design, etc.).
- `PLAN_UPDATES.md` — drift log against the original plan in `/root/.claude/plans/i-m-interested-in-implementing-unified-parasol.md`. Read when a behaviour seems to contradict the plan.
- `README.md` — user-facing build, OAuth client-ID setup, milestone roadmap.
- `.github/workflows/desktop-native.yml` — what CI actually runs (the source of truth for the pre-merge gate).
