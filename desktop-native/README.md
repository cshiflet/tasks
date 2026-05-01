# tasks-desktop — native desktop client (Rust + Qt 6)

Native desktop companion for Tasks.org, following the plan at
`/root/.claude/plans/i-m-interested-in-implementing-unified-parasol.md`.

**Status:** Milestone 1 — read-only companion. The `tasks-core` crate
opens the SQLite database the Android app writes and runs the same
task-list queries as the Android client (recursive CTE, sort/group
helpers, PermaSql placeholder expansion). The `tasks-ui` crate ships
a minimal Qt 6 / QML shell via `cxx-qt`, plus a `--cli` fallback mode
for headless smoke tests.

> ⚠️ Not to be confused with the `desktop/` directory on the
> `jetpack-desktop` branch (formerly `desktop`), which is a deprecated
> Compose-for-Desktop JVM client. See `CLAUDE.md` at the repo root.

## Build & run

See **[`BUILD.md`](BUILD.md)** for the canonical setup guide (Docker,
bare Ubuntu, Fedora/Arch, macOS, Windows), the dev loop, runtime env
vars, and troubleshooting.

Quick start with Docker:

```sh
docker build -t tasks-dev -f desktop-native/docker/Dockerfile.dev .
docker run --rm -it -v "$PWD":/workspace -w /workspace/desktop-native tasks-dev \
    bash -c 'cargo test --workspace && QT_QPA_PLATFORM=offscreen cargo run -p tasks-ui'
```

## Crate layout

```
desktop-native/
  Cargo.toml              # cargo workspace
  crates/
    tasks-core/           # pure Rust: models, DB open, queries, watcher
    tasks-ui/             # cxx-qt QObject + QML shell
      cxx/                # hand-written C++ shims (e.g. TaskListModelBase)
      qml/                # QML: Main.qml + three panes + PriorityDot
  docker/                 # reproducible toolchain recipes
  resources/              # icons, translations (stubs)
  packaging/{linux,macos,windows}/  # packaging stubs for future release work
```

## Schema pinning

`tasks_core::db::PINNED_SCHEMA_VERSION` and `PINNED_IDENTITY_HASH` must
match a snapshot in
`data/schemas/org.tasks.data.db.Database/<version>.json`. When upstream
Room migrations land, bump both constants together and re-run
`cargo test`. A CI job (`tests/schema_guard.rs`) flags drift on PRs
that touch either file.

## Roadmap

Milestone 1 (read-only companion):

- [x] Workspace scaffolding
- [x] Task model + read-only SQLite open with identity-hash verification
- [x] Minimal Active / Today filters
- [x] Filesystem watcher (debounced)
- [x] Remaining entities (Tag, Filter, Place, CaldavCalendar, Alarm, …)
- [x] Recursive `TaskListQuery` port (SortHelper + QueryPreferences +
      PermaSql)
- [x] Non-recursive path (`AstridOrderingFilter` / `RecentlyModifiedFilter`)
- [x] Qt 6 / QML shell via cxx-qt
- [x] Three-pane layout (`SidebarPane.qml` / `TaskListPane.qml` /
      `TaskDetailPane.qml`)
- [x] File picker (QML `FileDialog`)
- [x] OS dark-mode follow (`Material.theme: Material.System`)
- [x] Filesystem-watcher → UI refresh (auto-reload on DB change)
- [x] Synthesized fixture DB integration tests
- [x] Reproducible toolchain (`docker/Dockerfile.dev`)
- [x] Managed `tasks.db` at default OS data dir; creates empty schema
      on first launch
- [x] GitHub Actions release workflow (tag `desktop-native-v*` →
      draft release with Linux .tar.gz + macOS .dmg + Windows .zip
      artefacts)

Milestone 1.5 (bridge to writes):

- [x] JSON import from Tasks.org's Android backup format
      (File → Import in the toolbar; `tasks_core::import`)
- [x] Parent-child subtask re-linking on import
      (walks `caldavTasks.remoteParent` → `remoteId` and backfills
      `tasks.parent`; orphans whose parent UID isn't in the backup
      stay flat)
- [ ] Real Android-captured fixture DB for end-to-end tests
      *(optional — kept around as a convenience for debugging, not a
      blocker. Cross-device data exchange is JSON export/import and
      CalDAV/EteSync sync, not copying the SQLite file; see
      `PLAN_UPDATES.md §6.6`.)*

Milestone 2 (writes):

- [x] Click-to-complete + click-to-delete (soft). Each write opens a
      short-lived read-write SQLite connection in
      `tasks_core::write`, keeping the GUI's read-only handle intact.
- [x] Recurrence summary in the detail pane — `humanize_rrule` turns
      `FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE,FR` into
      "Every other week on Mon, Wed, Fri"; repeat-from-completion
      tasks get a `(from completion)` suffix. Complex rule parts
      (BYMONTHDAY, BYSETPOS, positional BYDAY) are dropped silently;
      full Android-parity RRULE rendering is a later pass.
- [x] Task edit dialog (title / notes / due / priority / hide-until /
      tags / reminders / location / parent / timer / recurrence).
- [ ] Add-new-task + bulk complete + undo/redo
- [ ] Recurring-task next-occurrence rescheduling on complete
      (needs RRULE parsing + timezone-aware dates)
- [ ] User-editable preferences panel (sort mode / grouping /
      show completed+hidden)
- [ ] `QAbstractListModel` with per-row roles (scaffolding committed
      in `cxx/task_list_model_base.h`; bridge/QML wiring pending)
- [ ] Full Android-parity RRULE humanisation (port of
      `RepeatRuleToString`: positional BYDAY, BYMONTHDAY, locale-aware
      weekday names, etc.)
- *Per-task color — **not applicable**: the Room schema at v92 has
  no `tasks.color` column. Android renders row color from the
  owning CalDAV list (`cdl_color`) or tags (`tagdata.color`), both
  of which are edited in their own flows.*

Milestone 2.5 (OS-native reminders):

- [ ] libnotify adapter on Linux
- [ ] NSUserNotificationCenter (or the Cocoa `UNUserNotification`
      replacement) on macOS
- [ ] WinRT Toast on Windows
- [ ] Reminder scheduler backed by `alarms.time` + `alarms.type`
      (shared between the three platform adapters)

Milestone 3 (CalDAV sync — the authoritative data-in path):

- [ ] `tasks-sync` crate with a `Provider` trait the UI speaks to
      (discover / list / push / pull). Implementations under
      `tasks-sync/src/providers/…`.
- [ ] CalDAV provider: service discovery via `.well-known/caldav`,
      PROPFIND / REPORT / PUT / DELETE over HTTP + WebDAV
      (stack: `reqwest` + `quick-xml` + a lightweight `ical` parser
      or a port of the relevant `libical` bits).
- [ ] iCalendar (VTODO) serialization — read *and* write, with
      RRULE / EXDATE / VALARM round-tripping.
- [ ] Auth: Basic + Digest; OAuth2 for Fastmail / iCloud where
      applicable.
- [ ] Sync engine: ctag-gated incremental pulls, etag-gated push
      merge, conflict detection, move-between-lists semantics.
- [ ] Account-management UI (add account, assign lists to CalDAV
      calendars, trigger sync).
- [ ] Integration tests against a Radicale instance in CI.

Milestone 4 (Google Tasks + Microsoft To Do):

- [ ] Google Tasks provider: REST API v1, loopback-OAuth2 flow
      (no embedded webview — system browser + `http://localhost` or
      `http://127.0.0.1` redirect).
- [ ] Microsoft To Do provider: Microsoft Graph / To Do REST,
      same loopback-OAuth2 shape.
- [ ] Shared token cache using `secret-service` / macOS Keychain /
      Windows Credential Manager, abstracted behind a
      `TokenStore` trait.
- [ ] Per-task `remoteId` semantics aligned with the existing
      `caldav_tasks` table shape so the three providers can
      coexist on one desktop install.

Milestone 5 (EteSync):

- [ ] `libetebase` FFI dependency (the Rust crate is a thin wrapper
      over the shared library; first-party Rust, no foreign sync
      glue).
- [ ] Collection / item layout mapped onto the Room task schema.
- [ ] Zero-knowledge key handling (password → login / encryption
      key derivation).

Milestone 6 (parity polish):

- [ ] Geofencing on desktop platforms — feasibility research first
      (QtPositioning is not in cxx-qt-lib; would need a hand-rolled
      bridge, or delegate to OS-native APIs: `CLLocationManager` on
      macOS, `Windows.Devices.Geolocation` on Windows, GeoClue2
      / `libgeoclue` on Linux).
- [ ] Widgets-equivalent quick-add (KDE Plasma applet, macOS
      widget extension, Windows tray).
- [ ] Automation hooks (D-Bus on Linux, URL schemes on macOS,
      IPC on Windows) as the desktop equivalents of Android's
      Tasker plug-in surface.

Milestone 7 (packaging):

- [ ] Linux: AppImage + Flatpak (the Flatpak manifest goes into
      `packaging/linux/`).
- [ ] macOS: notarized `.app` bundle + signed DMG (codesign + notary
      scripts live in `packaging/macos/`).
- [ ] Windows: MSIX + a fallback NSIS installer
      (`packaging/windows/`).

Full detail in the plan file and in `desktop-native/PLAN_UPDATES.md`.

## See also

- [`BUILD.md`](BUILD.md) — build, test, and troubleshooting.
- [`DECISIONS.md`](DECISIONS.md) — non-obvious technical choices and
  the reasoning behind them.
- [`PLAN_UPDATES.md`](PLAN_UPDATES.md) — how the delivered work drifted
  from the original plan, and what amendments follow.
