# tasks-desktop — native desktop client (Rust + Qt 6)

Native desktop companion for Tasks.org, following the plan at
`/root/.claude/plans/i-m-interested-in-implementing-unified-parasol.md`.

**Status:** Milestone 1 — read-only companion. The `tasks-core` crate opens
the same SQLite database the Android app writes and can run a small set of
task-list queries. The `tasks-ui` crate currently ships only a CLI entry
point; the Qt/QML front-end is scaffolded but not yet wired to cxx-qt.

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

## Build — CLI runner (uses core)

```sh
cd desktop-native
cargo run -p tasks-ui -- /path/to/tasks.db
```

The runner opens the database read-only, verifies the Room identity hash
(pinned in `tasks_core::db`), and prints the Active filter.

## Build — full Qt UI (pending)

The Qt front-end requires Qt 6.5+ and `cxx-qt-build`. Platform setup:

- **Linux**: install `qt6-base-dev`, `qt6-declarative-dev`,
  `qt6-tools-dev-tools` (Debian/Ubuntu) or `qt6-qtbase-devel`,
  `qt6-qtdeclarative-devel` (Fedora).
- **macOS**: `brew install qt@6` and `export CMAKE_PREFIX_PATH="$(brew --prefix qt@6)"`.
- **Windows**: install Qt 6.5+ via the Qt Online Installer and put
  `C:\Qt\6.5.x\msvc2022_64\bin` on `PATH`.

Once Qt is present, enable the cxx-qt deps in `crates/tasks-ui/Cargo.toml`
and uncomment the bridge module in `tasks-ui/src/`. This gate is in place
because committing unverified cxx-qt code would block CI on contributors
who don't have Qt installed yet.

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
- [ ] Remaining entities (Tag, Filter, Place, CaldavCalendar, Alarm)
- [ ] Full port of `kmp/TaskListQuery*.kt`
- [ ] Qt/QML three-pane UI via cxx-qt
- [ ] Packaging (AppImage / Flatpak, notarized `.app` + DMG, MSIX)

Later milestones: write path + reminder scheduling, CalDAV sync,
Google Tasks / Microsoft To Do, EteSync, geofencing + widget equivalents.
Full detail in the plan file.
