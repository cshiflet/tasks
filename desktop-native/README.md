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
- [ ] Parent-child subtask re-linking on import
      (walk `caldavTasks.remoteParent` → `remoteId` and backfill
      `tasks.parent`)
- [ ] Real Android-captured fixture DB for end-to-end tests

Milestone 2 (writes):

- [ ] `QAbstractListModel` with per-row roles (scaffolding committed
      in `cxx/task_list_model_base.h`; bridge/QML wiring pending)
- [ ] Click-to-complete + swipe-to-delete with
      recurring-task next-occurrence rescheduling
- [ ] Task edit dialog (title / notes / due / priority / hide-until)
- [ ] Add-new-task, bulk complete, undo/redo
- [ ] User-editable preferences panel (sort mode / grouping /
      show completed+hidden)

Milestone 2.5: OS-native reminder notifications (libnotify on
Linux, NSUserNotification on macOS, WinRT Toast on Windows).

Later milestones: CalDAV sync, Google Tasks / Microsoft To Do,
EteSync, geofencing + widgets, packaging (AppImage / Flatpak,
notarized `.app` + DMG, MSIX). Full detail in the plan file and in
`desktop-native/PLAN_UPDATES.md`.

## See also

- [`BUILD.md`](BUILD.md) — build, test, and troubleshooting.
- [`DECISIONS.md`](DECISIONS.md) — non-obvious technical choices and
  the reasoning behind them.
- [`PLAN_UPDATES.md`](PLAN_UPDATES.md) — how the delivered work drifted
  from the original plan, and what amendments follow.
