# Plan updates — lessons learned during Milestone 1 implementation

This document reconciles the original implementation plan at
`/root/.claude/plans/i-m-interested-in-implementing-unified-parasol.md`
against the work actually delivered on branch
`claude/native-desktop-client-5FFnd`, calls out where experience forced
a course correction, and proposes updates for the remaining milestones.

Read alongside `DECISIONS.md`, which records *why* specific choices were
made; this file records *what changed vs the plan* and what the plan
should say going forward.

## 1. What the plan got right

- **Rust + Qt 6 via cxx-qt** survived contact with the implementation.
  The Rust side proved straightforward to bring online; `cxx-qt 0.7`
  was a good fit; Qt 6.4 on Ubuntu 24.04 linked without incident.
- **Schema pinning with an identity-hash check** is genuinely
  protective. The hash-drift CI guard is cheap and catches real bugs
  before they reach users.
- **Port the `kmp/` query builders rather than share a JVM runtime.**
  The Kotlin-to-Rust translation was ~450 lines for `SortHelper`,
  `TaskListQueryRecursive`, and `TaskListQueryNonRecursive` combined;
  a JNI bridge would have been an order of magnitude more code and
  would have re-introduced a JVM dependency the user explicitly
  rejected.
- **Phased delivery.** Shipping Milestone 1 as read-only kept every
  change testable without touching the Android app's write path,
  which turned out to be crucial when the first GUI smoke tests
  surfaced build-system issues.

## 2. Where the plan drifted — and why

### 2.1 `QAbstractListModel` deferred, parallel Q_PROPERTYs instead

**Plan said (Milestone 1, step 8):** "drop a proper
`QAbstractListModel` in for per-row roles."

**Delivered:** a `TaskListViewModel` that exposes parallel
`Q_PROPERTY`s (`titles`, `taskIds`, `indents`, `completedFlags`,
`dueLabels`, `priorities`) which QML delegates index by row.

**Why:** `cxx-qt-lib 0.7` doesn't ship a ready-made
`QAbstractListModel` adapter; subclassing via `#[base =
QAbstractListModel]` requires hand-rolled C++ glue for `rowCount`,
`data`, and `roleNames`, roughly 150 lines for what is still a
read-only model. The parallel-property shape covers every field a
ListView delegate needs with ~zero new FFI surface. When the write
path lands and per-row mutation signals become valuable, the adapter
is the right place to invest; until then it's over-engineering.

**Plan update:** replace "drop a proper QAbstractListModel in" with
"promote the parallel Q_PROPERTY model to a `QAbstractListModel`
once the write path lands, so `beginInsertRows`/`dataChanged` can
drive fine-grained UI updates."

### 2.2 MSRV bumped to 1.82

**Plan said:** (implicit) "Rust stable, whatever the default edition."

**Delivered:** workspace `rust-version = "1.82"`.

**Why:** `cxx-qt 0.7.3`'s generated code uses APIs stable since
Rust 1.82. Downgrading cxx-qt would cost more than bumping MSRV.
Documented in `DECISIONS.md` #11.

**Plan update:** state the minimum explicitly.

### 2.3 Custom-filter dispatch needed an explicit executor

**Plan implied:** "the UI layer pairs `build_recursive_query` with a
user's `QueryPreferences` and the active `QueryFilter`."

**Delivered:** `tasks_core::query::run_by_filter_id(db, id, now_ms)`
that accepts a tagged string identifier (`__all__`, `__today__`,
`__recent__`, `caldav:<uuid>`, `filter:<row_id>`) and returns the
matching rows. This landed because the sidebar needed a single
entry point the QML side could call with a `QString`, and because
keeping `rusqlite` out of `tasks-ui` meant factoring the prepared-
statement path into `tasks-core`.

**Plan update:** document the filter-identifier grammar (see
`DECISIONS.md` #9) as part of Milestone 1's UI contract so the
eventual `QAbstractListModel` uses the same vocabulary.

### 2.4 Date/time formatting is UTC-only for now

**Plan said (implicitly):** human-readable dates in the detail pane.

**Delivered:** `format_due_label` returns UTC ISO strings
(`YYYY-MM-DD` or `YYYY-MM-DD HH:MM`). No timezone handling.

**Why:** bringing in `time 0.3` or `chrono` for Milestone 1 would
grow the dep tree for a cosmetic feature. Howard Hinnant's
algorithm in 20 lines of Rust gets the date right.

**Plan update:** Milestone 2 (writes) should pull in a full date/
time crate at the same time it pulls in RRULE parsing, since both
need timezone-aware computation.

### 2.5 Non-recursive path ported early

**Plan said (Milestone 1):** recursive path only; non-recursive was
marked as an explicit deferral.

**Delivered:** both recursive and non-recursive, plus a dispatcher
matching Kotlin's `when` cascade.

**Why:** the `RecentlyModifiedFilter` sidebar row wanted the
non-recursive path anyway, and once the Kotlin original was open
it was quicker to port both than to stub one. The cascade's bug-
prone case (AstridOrderingFilter without astrid-sort → recursive)
shook out in self-review and now has regression tests.

**Plan update:** strike the non-recursive deferral from Milestone 1.

## 3. Items still on the Milestone 1 roadmap

- **Filesystem watcher → UI refresh.** The `notify`-based watcher
  exists in `tasks-core::watch`, but is not yet wired to the cxx-qt
  bridge. Needs a signal emission from the Rust side so QML can
  observe changes without polling.
- **File picker.** The current QML shell accepts a path string;
  `QFileDialog` integration is still pending.
- **OS theme follow.** `Main.qml` doesn't react to the system's
  dark-mode toggle yet.
- **Manual QA against a real Android DB.** See §5.

## 4. Updates to later milestones

- **Milestone 2 (writes + reminders):** promote the view model to
  `QAbstractListModel` at this milestone, not later. Per-row updates
  become cheap and legible once we can emit `dataChanged(index)`.
- **Milestone 3 (CalDAV):** `libcurl + libxml2 + libical` is still
  the right substrate; cxx-qt is orthogonal. No change.
- **Milestone 4 (Google Tasks + MS Todo):** the plan's warning about
  `QtWebEngine` binary bloat stands — doubly so now that cxx-qt-lib
  itself already ~triples the compiled binary vs a pure-Rust build.
  Strongly prefer the system-browser + loopback OAuth flow.
- **Milestone 5 (EteSync):** no changes. Plan for a C-API FFI to
  `libetebase` stands.
- **Milestone 6 (parity polish):** geofencing is blocked on
  `QtPositioning`, which isn't in the cxx-qt-lib surface. Call out
  that it needs either a hand-rolled bridge or a switch to a
  different positioning crate.

## 5. Lessons from the code review

An independent review of the delivered work caught three MAJOR issues
that shipped under the initial test coverage, which is worth absorbing
into the plan as a validation-gap pattern:

1. **`show_hidden` was a silent no-op.** The literal `<=?` from the
   Kotlin regex was copied into a plain `str::replace`, which
   matches literally (including the `?`) and therefore never matches
   the actual SQL emitted elsewhere in the module. No test exercised
   `show_hidden`, so the failure was invisible.
2. **Sort-direction inversion for `SORT_MODIFIED` / `SORT_CREATED`.**
   The Kotlin `orderForSortType` + `reverse()` dance collapses to
   "final direction = `preferences.sortAscending`" regardless of the
   sort type's natural direction. The Rust port preserved the
   intermediate natural-direction flag and XOR'd, which inverted the
   final result for the two DESC-natural sort types.
3. **`SORT_LIST` used the wrong column and dropped its secondary.**
   Kotlin's `ORDER_LIST` orders by `UPPER(cdl_order)` with
   `cdl_name` as the secondary; the Rust port emitted
   `UPPER(cdl_name)` as the primary and dropped the secondary
   entirely.

All three were fixed with regression tests. The meta-lesson is that
the existing snapshot-style assertions (*"does the SQL contain
WITH RECURSIVE?"*) were too coarse to catch behavioural divergence.
**Plan update: every sort-mode constant (SORT_ALPHA, SORT_DUE,
SORT_START, SORT_IMPORTANCE, SORT_MODIFIED, SORT_CREATED, SORT_LIST,
SORT_COMPLETED, SORT_CALDAV, SORT_GTASKS) should have at least one
test that pins both the primary expression and the direction logic
for both `sort_ascending=true` and `sort_ascending=false`.** That
test matrix is ~20 cases and should have been in the initial port.

Minor findings applied alongside the fixes:
- `build_sidebar` now logs errors via `tracing::warn!` instead of
  silently swallowing them.
- `run_by_filter_id` logs invalid `filter:<id>` strings instead of
  treating them as `filter:0`.
- `show_completed` no longer emits spaced variants that Kotlin's
  regex wouldn't match.
- `non_recursive` builds the completed-at-bottom prelude only when
  the flag is on, rather than emitting a stray double space.

A deferred minor finding (redundant DB re-open in
`bridge.rs::reload_active_filter`) stays open — see §3 in this doc;
it gates on moving to a `QAbstractListModel` where the DB handle
would naturally live alongside the model's state.

## 6. Validation gaps I cannot close in the current environment

The following require work outside the sandbox and are explicitly
deferred to the user:

- **End-to-end test against an actual Android-produced SQLite file**
  at the pinned schema version. The test fixtures I authored cover
  column shape but not realistic row density, subtask graphs, or
  CalDAV metadata coverage.
- **Visual review of the three-pane QML layout** on each target OS
  (Linux/macOS/Windows). `QT_QPA_PLATFORM=offscreen` in CI confirms
  the QML graph parses; it does not exercise the compositor or
  native look-and-feel.
- **Sync-server integration tests** for CalDAV/Google/Microsoft/
  Etebase. Need a Radicale/Nextcloud/Fastmail/mock instance in CI.
  The original plan called these out; they're still pending.
- **Packaging dry runs** (AppImage, Flatpak, notarized .app, MSIX).
  Requires signing identities and distribution infrastructure that
  don't belong in the general-purpose CI image.

These items don't invalidate the delivered work — they're the natural
handoff boundary between "Milestone 1 code complete" and "Milestone 1
shippable."

## 6.5. Round-2 review findings + fixes

A second independent review of this document (and the delivered code)
surfaced several items the first-round review missed. All the
actionable ones landed on the branch alongside this revision:

1. **Today-window used UTC midnight, not local midnight.** A user in
   UTC-8 would see the Today filter offset by up to 8 hours. §2.4 had
   framed timezones as "cosmetic polish for a later milestone" — that
   was wrong. Fix: `today_window_ms(now_ms, local_offset_secs)` takes
   the caller's offset; `run_by_filter_id` accepts it as a parameter;
   the cxx-qt bridge sources it via `QDateTime::offsetFromUtc()`.
   Three new unit tests cover UTC, west-of-UTC, and east-of-UTC
   anchoring.
2. **`run_by_filter_id` used `QueryPreferences::default()` regardless
   of the user's settings.** Not a bug today because no UI exists to
   edit preferences, but a future-bug-in-waiting. Fix: accept
   `prefs: &QueryPreferences` as a parameter; the bridge stores a
   `preferences: QueryPreferences` field and passes it through. The
   preferences-panel UI still has to land before the field is used;
   that's noted in §3.
3. **Cached `Database` handle in the view model.** Previously
   `reload_active_filter` reopened the DB on every filter navigation
   (extra identity-hash check, extra SQLite handshake). Now `Database`
   lives on the view model alongside `db_path`; the handle is
   refreshed only on `openDatabase(path)`.
4. **`QueryFilter::Caldav` arm in `build_non_recursive_query`
   silently returned an empty string.** Replaced with a
   `debug_assert!(false, …)` + `tracing::error!` so the footgun is
   loud instead of silent; the dispatcher path never reaches the arm.
5. **Sort-mode test coverage expanded.** `every_sort_mode_emits_expected_direction`
   iterates the full set of non-recursive sort constants (ALPHA, DUE,
   START, IMPORTANCE, MODIFIED, CREATED, LIST, COMPLETED, GTASKS,
   CALDAV) against both `sort_ascending=true` and `=false`, asserting
   on both the expression fragment and the emitted direction.
6. **CI expanded to a three-OS matrix.** `check-linux` (fmt + clippy
   + test + GUI smoke under `QT_QPA_PLATFORM=offscreen`),
   `check-macos` (test + GUI smoke via Qt installed through
   `jurplel/install-qt-action`), `check-windows` (test only, GUI
   smoke deferred — see workflow comment). This closes the slippage
   the reviewer correctly identified against the original plan's
   "Linux + macOS + Windows, single Qt codebase" promise.

Items from the round-2 review that remain open (intentionally):

- **Promote `FilterId` sidebar grammar to a Rust enum.** Cleanup,
  not correctness. Natural to do alongside the `QAbstractListModel`
  promotion in Milestone 2.
- **FTS / Room type-converter audit.** The schema JSON at v92 has
  no FTS tables and no type converters affecting the columns we
  read (verified by skimming `data/schemas/.../92.json`); no action
  needed now, but worth re-verifying when the pinned version bumps.
- **PermaSQL placeholder + embedded quote test.** The only sites
  that interpolate strings are `caldav_parent_query` (uuid, tested)
  and saved-filter SQL (already treated as authoritative). The
  attack surface is smaller than the round-2 review implied.

## 6.6. Corrected premise: SQLite is local-per-client, not shared

The original plan framed Milestone 1 as "open the same SQLite database
the Android app writes (delivered over Syncthing, iCloud Drive,
OneDrive, USB, etc.)" That premise was wrong. Verified against the
Android app source:

1. **The DB lives in Android's per-app sandbox.** `ProductionModule.kt`
   line 44 calls `context.getDatabasePath(Database.NAME)`, which
   resolves to `/data/data/org.tasks/databases/*`. That path is not
   reachable by other apps, other users, or cloud-sync tools without
   root access.
2. **Tasks.org's own backup format is JSON, not SQLite.**
   `TasksJsonExporter.kt` serialises entities via
   `kotlinx.serialization.json`. The SQLite file itself is never
   part of the user-facing export surface.
3. **Cross-device data flow is via sync backends** (CalDAV, Google
   Tasks, Microsoft To Do, EteSync), not via shared local storage.
   Each installed client keeps its own Room DB and reconciles
   against a remote.

Implications:

- **Concurrent-writer contention is a non-concern.** The desktop
  client owns its SQLite file. `db::Database::open_read_only`
  previously held a 500 ms `busy_timeout` to tolerate a
  simultaneous-writer Android. That comment and duration have been
  dropped — a 50 ms defensive belt remains only in case a second
  desktop process briefly contends.
- **Milestone 1's "read-only companion" story needs a clearer
  data-in path.** In the current environment the fixtures are
  synthetic; a realistic Milestone 1 user would need either:
  - an **ADB pull** of the Android DB file (advanced users on a
    rooted device or with `adb backup` tooling) — good for
    debugging, not a shipping UX.
  - a **JSON-import path** that reads the existing Android backup
    JSON and materialises a local SQLite at the pinned schema.
    This is the natural Milestone 1.5 / Milestone 2 bridge.
  - a **fresh DB populated by the desktop's own CalDAV sync**
    (Milestone 3), at which point the "read-only companion" framing
    gets retired entirely in favour of "first-class client".

The originally-planned `## Risks` item #3 ("SQLite concurrency when
Android is writing while desktop reads. Mitigation: open with
WAL-friendly settings…") should be struck from the plan — the
scenario it mitigates doesn't exist.

Plan amendments this implies:

- Remove Risk #3 from the plan.
- Re-scope Milestone 1 delivery: the query and view-model layers are
  useful on their own (they'll back every subsequent milestone), but
  the end-user-visible feature depends on a data-in path that the
  current plan doesn't specify.
- Add a new Milestone 1.5 or merge into Milestone 2: **JSON-import
  from Tasks.org's backup format**. The code for the Android side
  lives at `app/src/main/java/org/tasks/backup/TasksJsonExporter.kt`
  (writer) and `TasksJsonImporter.kt` (reader); the Rust port can
  mirror the serialized shape.
- Reframe Milestone 3 (CalDAV) and onwards as **the authoritative
  data-in path**. The desktop's SQLite is seeded either by JSON
  import or by a fresh sync against the user's existing CalDAV
  account — never by sharing Android's sandbox file.

## 6.7. Milestone 1 shipped; Milestone 1.5 (JSON import) landed

Manual testing on Windows confirmed Milestone 1 behaves end-to-end
— the managed `tasks.db` auto-creates at the OS data dir on first
launch, the three-pane UI renders, and the QML bindings now
resolve to real values (caught and fixed the cxx-qt `auto_cxx_name`
gotcha while verifying).

Follow-up work, in the order the user prioritised:

- **Milestone 1.5 — JSON import (shipped).** Ports
  `org.tasks.backup.TasksJsonExporter`'s output format into a
  `tasks_core::import` module that materialises Tasks.org's
  Android-side backup into the desktop's own SQLite. The entry
  point lives in `Main.qml`'s toolbar as an "Import backup"
  button; it opens a `FileDialog` scoped to `*.json`, hands the
  selected path to the bridge, and the bridge invokes the importer
  on the currently-open DB inside a single transaction.

  Deliberate limitations captured in module docs:

  * Task.parent is `@Transient` in Kotlin and therefore absent from
    the JSON; subtasks import as flat tasks until we add a
    re-linking pass that walks `caldavTasks.remoteParent` →
    `remoteId`.
  * Attachments (file-content) and user-activity comments are
    counted but skipped — the export carries their metadata but
    the content lives in URI references.
  * Task-list metadata + Astrid-era legacy locations are not
    imported; rare on modern installs.

  Three integration tests pin the behaviour:
  `import_backup_populates_every_entity` (including re-running the
  same backup twice to prove `INSERT OR REPLACE` on `tasks.remoteId`
  keeps things idempotent), `import_rolls_back_on_parse_error`,
  and `import_missing_file_reports_io_error`.

- **Milestone 2 — writes (in progress).** Per the user's selection:
  complete + delete + full task-edit dialog, promoted alongside
  the `QAbstractListModel` upgrade (scaffolding already in tree at
  `cxx/task_list_model_base.h`). The parallel-Q_PROPERTY shape was
  right for read-only but won't scale to per-row dataChanged
  signals.

  **Phase A (shipped):** click-to-complete and soft-delete.
  `tasks_core::write` exposes `set_task_completion` and
  `set_task_deleted`, each of which opens its own short-lived
  read-write SQLite connection, runs a one-statement transaction,
  and closes. The read-only handle the GUI holds stays valid
  throughout — SQLite's per-connection locking is enough without any
  coordination. The bridge gets `toggleTaskCompletion(id, bool)` and
  `deleteSelectedTask()` Q_INVOKABLEs; the QML layer surfaces a
  CheckBox on each list row and a Delete button plus confirm dialog
  in the detail pane. Three integration tests cover
  complete/uncomplete, soft-delete, and unknown-id handling.

  Deliberately deferred to Phase B:

  * **Recurring-task rescheduling on complete.** Android's behaviour
    is to advance `dueDate` to the next RRULE occurrence rather than
    set `completed = now`. That requires an RRULE parser (candidate:
    the `rrule` crate, which we haven't pulled in yet) and a
    timezone-aware date crate. For Phase A, completing a recurring
    task just stamps `completed` — good enough for verification, not
    behaviour-equivalent to the Android client.
  * **Edit dialog.** Title/notes/due/priority/hide-until comes with
    Phase B.
  * **Add-new-task and undo/redo.** Phase C.
  * **QAbstractListModel promotion.** Phase D; the parallel-property
    shape still works for Phase A/B since each completion toggle
    already reloads the whole list via `reload_active_filter`.

- **Milestone 2.5 — OS-native reminders.** Deferred from M2 to
  keep the write-path validation tractable. libnotify on Linux,
  `NSUserNotification` on macOS, Windows Toast on Windows.

- **Milestone 3 — CalDAV sync.** Unchanged; still the right
  substrate.

## 7. Bottom line

The query-builder and view-model layers of Milestone 1 are
**code-complete and under meaningful test**. The delivered work
covers the full recursive + non-recursive query cascade, every
round-tripped entity, timezone-correct day windows, and CI coverage
on Linux + macOS + Windows.

The UX layer of Milestone 1 is **~70 % done**:

- ✅ Three-pane layout with filter switching and task detail.
- ✅ Sidebar populated from CalDAV lists + saved filters.
- ❌ File picker (QFileDialog integration).
- ❌ OS dark-mode follow.
- ❌ Filesystem-watcher → UI refresh signal (core side exists,
  bridge side doesn't).
- ❌ Real Android-captured fixture DB for end-to-end tests.
- ❌ User-editable preferences panel (sort mode, grouping,
  show completed/hidden).

The dependency risks (iCloud CalDAV quirks, QtWebEngine bloat,
schema drift, `libetebase` drift) called out in the original
`## Risks` section remain accurate.

Five material updates to the plan itself:

1. The parallel-Q_PROPERTY model shape replaces the
   `QAbstractListModel` goalpost for Milestone 1 and promotes it to
   Milestone 2 instead.
2. Rust MSRV pinned at 1.82.
3. Non-recursive query path was moved forward from "later milestone"
   into Milestone 1 without cost.
4. **Query-builder parity testing must be comprehensive up front.**
   The original plan treated *"port and then spot-check"* as
   adequate; the first code review proved otherwise. Going forward,
   every sort-mode constant gets a paired ASC/DESC test, and every
   predicate-rewrite helper (`show_hidden`, `show_completed`, future
   `removeOrder`) gets an explicit before/after fixture.
5. **Timezone handling is Milestone 1 scope, not a deferral.** The
   second review caught the UTC-midnight Today-window bug; the fix
   is on the branch. Any future filter that derives a time window
   from "now" must take a local-offset parameter from the caller.
