# tasks-desktop — native desktop client (Rust + Qt 6)

Native desktop companion for Tasks.org, following the plan at
`/root/.claude/plans/i-m-interested-in-implementing-unified-parasol.md`.

**Status:** Milestone 1 — read-only companion. The `tasks-core` crate opens
the same SQLite database the Android app writes and runs the same task-list
queries as the Android client (recursive CTE, sort/group helpers, PermaSql
placeholder expansion). The `tasks-ui` crate ships a minimal Qt 6 / QML
shell via `cxx-qt`, plus a `--cli` fallback mode for headless smoke tests.

> ⚠️ Not to be confused with the `desktop/` directory on the
> `jetpack-desktop` branch (formerly `desktop`), which is a Compose-for-Desktop
> JVM client and is deprecated. See `CLAUDE.md` at the repo root.

## Layout

```
desktop-native/
  Cargo.toml              # workspace
  crates/
    tasks-core/           # pure Rust: models, DB open, queries, fs watcher
    tasks-ui/             # Qt/QML front-end (cxx-qt integration pending)
  resources/              # shared assets (icons, translations)
  packaging/{linux,macos,windows}/
```

## Build — core only

Qt is not required for the core crate:

```sh
cd desktop-native
cargo build -p tasks-core
cargo test  -p tasks-core
```

## Build — CLI runner (uses core; no Qt required at runtime)

```sh
cd desktop-native
cargo run -p tasks-ui -- --cli /path/to/tasks.db
```

Opens the database read-only, verifies the Room identity hash (pinned in
`tasks_core::db`), and prints the Active filter.

## Build — Qt 6 GUI

The GUI requires Qt 6.4+ at build and run time.

- **Linux**: install `qt6-base-dev`, `qt6-declarative-dev`, `qt6-tools-dev`,
  `qml6-module-qtquick{,-controls,-layouts,-window}`, `libqt6svg6-dev`,
  `pkg-config` (Debian/Ubuntu) or their Fedora/Arch equivalents.
- **macOS**: `brew install qt@6` and
  `export CMAKE_PREFIX_PATH="$(brew --prefix qt@6)"`.
- **Windows**: install Qt 6.4+ via the Qt Online Installer and put
  `C:\Qt\6.x\msvc2022_64\bin` on `PATH`.

```sh
cd desktop-native
cargo run -p tasks-ui
```

On headless CI machines, set `QT_QPA_PLATFORM=offscreen` so the window
opens without a display server.

## Schema pinning

`tasks_core::db::PINNED_SCHEMA_VERSION` and `PINNED_IDENTITY_HASH` must
match a snapshot in
`data/schemas/org.tasks.data.db.Database/<version>.json`. When upstream
Room migrations land, bump both constants together and re-run
`cargo test`. A CI job will later diff the pinned hash against the newest
schema file and flag drift.

## Roadmap

Milestone 1 (in progress — this crate):

- [x] Workspace scaffolding
- [x] Task model + read-only SQLite open with identity-hash verification
- [x] Minimal Active / Today filters
- [x] Filesystem watcher (debounced)
- [x] Remaining entities (Tag, Filter, Place, CaldavCalendar, Alarm, …)
- [x] Recursive `TaskListQuery` port (SortHelper + QueryPreferences +
      PermaSql)
- [x] Qt 6 / QML shell via cxx-qt (first-cut: single-window task viewer)
- [ ] `QAbstractListModel` with per-row roles (title, due, priority,
      tags, indent)
- [ ] Three-pane layout (`SidebarPane.qml` / `TaskListPane.qml` /
      `TaskDetailPane.qml`)
- [ ] Non-recursive path (`AstridOrderingFilter` / `RecentlyModifiedFilter`)
- [ ] Packaging (AppImage / Flatpak, notarized `.app` + DMG, MSIX)

Later milestones: write path + reminder scheduling, CalDAV sync,
Google Tasks / Microsoft To Do, EteSync, geofencing + widget equivalents.
Full detail in the plan file.
