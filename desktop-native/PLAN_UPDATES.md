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

## 8. Milestone 2 shipped (writes, full edit dialog, add/edit/delete,
     preferences, advance-on-complete, humaniser polish)

Every line item under the Milestone-2-writes bullet list is either
landed or explicitly deferred with rationale. Shipped across Phases
A–C plus the post-review polish:

- **Phase A** — click-to-complete + soft-delete. `tasks_core::write`
  opens a short-lived read-write SQLite connection per write so the
  GUI's read-only handle stays intact.
- **Phase B** — task edit dialog (title / notes / due / priority /
  hide-until). `tasks_core::datetime` centralised the
  `YYYY-MM-DD [HH:MM]` ↔ epoch-ms round-trip with leap-aware day
  validation.
- **Phase C1–C8** — CalDAV list picker, tags multi-select,
  reminders (REL_END add + list + remove), location / geofence,
  parent task ComboBox, timer (estimate + elapsed) fields, and an
  inline recurrence editor (FREQ / INTERVAL / BYDAY / from-due-vs-
  completion). Per-task colour confirmed as not-in-schema; Android
  inherits it from the CalDAV list or tags.
- **Post-review round** — four parallel audits surfaced a handful
  of MAJORs (silenced tag-lookup errors, missing
  day-in-month rejection, stale catalog arrays after DB-open
  failure, QML array-bounds hygiene) — all fixed on branch.
  Three high-priority test-coverage gaps closed (cascade delete,
  unchanged-alarm preservation, parent cycle self-coercion).
- **Add-new-task + preferences** — `create_task` mints a task with
  a fresh UUID v4 as `tasks.remoteId` and, when the active filter
  is a CalDAV list, creates a `caldav_tasks` row with its own
  distinct UID. Preferences dialog binds to the existing
  `QueryPreferences` struct and reloads the filter on save.
- **Advance-on-complete** — completing a recurring task now slides
  `dueDate` forward by the rule's interval (FREQ / INTERVAL /
  BYDAY for WEEKLY; month-length clamping for MONTHLY / YEARLY)
  instead of stamping `completed`. Matches Android parity for the
  rule subset the desktop editor can construct.
- **RRULE humaniser polish** — positional BYDAY now reads as
  "last Friday", "first Monday"; BYMONTHDAY renders as
  "the 1st, 15th, last day" with teen-aware ordinal suffixes.

Deferred to later milestones (explicit):

- **Bulk-complete / undo-redo** — not in Phase C1–C8. A proper
  undo stack pairs naturally with the QAbstractListModel
  migration (Milestone 6, per roadmap).
- **QSettings persistence for preferences** — session-local for
  now. The Q_PROPERTY shape is ready for a persistence pass; just
  needs the on-save hook and an init at startup.
- **COUNT / UNTIL editing in the recurrence picker** — parsed
  but not editable; documented in the dialog footer.

## 9. Sync scaffolding (Milestones 3/4/5)

The `tasks-sync` crate skeleton lands on this branch with a
`Provider` trait, the shared value types (`AccountCredentials`,
`RemoteCalendar`, `RemoteTask`, `SyncError`), and stub provider
implementations for CalDAV, Google Tasks, Microsoft To Do, and
EteSync. Every network method returns
`SyncError::NotYetImplemented` so the UI can start speaking the
trait today while the individual providers fill in one at a time.

This is a deliberate architectural seam: the UI talks
`Box<dyn Provider>`, never provider-specific types, so the four
backends can land in any order and co-exist on one desktop
install without UI churn.

Per-provider dependency sets stay out of the crate's `Cargo.toml`
for now (no `reqwest`, `oauth2`, or `libetebase-rs` yet). Those
arrive with the first actual network commit per provider so the
skeleton build stays fast.

## 10. Sync-layer substrate (follow-up to §9)

On top of the trait/stub skeleton, the pure-logic half of every
sync path landed — everything that doesn't require a live server
or credentials can be tested today from fixture strings:

- **`tasks_sync::ical`** — RFC 5545 VTODO parser + serializer
  covering the subset Tasks.org emits/consumes. Line folding,
  TEXT escaping, VALARM offsets + absolute triggers,
  RELATED-TO=PARENT, CATEGORIES, CREATED/LAST-MODIFIED. Out of
  scope: VTIMEZONE/TZID, RECURRENCE-ID, EXDATE/RDATE,
  attachments.
- **`tasks_sync::convert`** — lossless VTodo ↔ RemoteTask
  translation. Priority buckets canonical-round-trip (HIGH→1,
  MEDIUM→5, LOW→9, NONE→0); `RemoteTask.due_has_time` was
  added so CalDAV `HH:00:00Z` dates don't collapse to date-only
  through the Tasks.org has-time-flag heuristic.
- **`tasks_sync::providers::caldav_xml`** — PROPFIND request
  templates for the service-discovery cascade + REPORT body for
  `calendar-query` VTODO listings. Quick-xml pull-parser turns
  multi-status responses into typed structs (calendar listing,
  task resource listing, principal). XML-escapes hrefs for
  `calendar-multiget`.
- **`tasks_sync::oauth`** — PKCE challenge generation (RFC 7636
  test-vector-validated), authorization URL construction,
  redirect parsing (with state CSRF check), token + refresh
  request body assembly. Sticks to `sha2` + `base64` +
  `percent-encoding` + `getrandom` so the full `oauth2` crate's
  tokio/reqwest defaults don't leak in.
- **`tasks_sync::loopback`** — OAuth redirect receiver. Binds
  127.0.0.1:0, hands the caller the port to embed in the
  redirect URI, blocks on a single incoming request, parses via
  `oauth::parse_redirect`, responds with a "you can close this
  window" page. Uses `std::net` only — no tokio, no reqwest.
- **`tasks_sync::providers::google_json`** +
  **`…::microsoft_json`** — REST payload shapes for the two
  OAuth providers, with RFC 3339 parsers that handle both Z
  suffixes and ±HH:MM offsets. Importance ↔ priority buckets
  for Microsoft (high/normal/low → HIGH/MEDIUM/LOW, unknown →
  NONE). Shared `OdataList<T>` envelope with `@odata.nextLink`
  pagination.
- **`tasks_sync::token_store`** — abstract `TokenStore` trait
  + `InMemoryTokenStore`. Keyed on `(ProviderKind, account)` so
  a user can connect two Google accounts side by side.
  OS-native keychain adapters (libsecret / Keychain / Credential
  Manager) land additively with the first real OAuth provider.
- **`tasks_sync::engine::SyncEngine`** — orchestrator. Pulls
  all calendars and tasks inside one rusqlite transaction
  ("remote wins" merge, preserving local-only tags/alarms/
  geofence); pushes dirty rows via the provider's `push_task`;
  surfaces `SyncError::Conflict` per-row without aborting the
  batch; tombstones tasks that disappear from a remote pull.
  `sync_now()` composes pull + push for the UI's "Sync now"
  button.

All exercised end-to-end from test code via a `MockProvider`
with in-memory calendar + task fixtures. The per-provider
implementations (actual reqwest/libcurl, etebase-rs FFI) will
plug in as thin wrappers over `oauth`, `loopback`, and the
JSON/XML helpers already here.

Dependencies pulled in so far: `quick-xml`, `sha2`, `base64`,
`getrandom`, `percent-encoding`, `serde_json`, `async-trait`,
`tokio` (dev-only). No reqwest, no oauth2 crate, no
libetebase-rs yet — those arrive with each provider's first
network commit.

## 11. All four sync providers wired (Milestones 3, 4, 5 — code complete)

Every provider stub in §9 is now a real implementation. Same
`Provider` trait surface; same `SyncEngine::sync_now()` still
composes them. Order in which they landed and what each added:

- **EteSync (Milestone 5).** Wraps `etebase` 0.5 via
  `tokio::task::spawn_blocking` since the crate is synchronous.
  `Account` is not Clone, so the session sits behind an
  `Arc<Mutex<Option<Account>>>` and every provider call clones
  the Arc into the spawn_blocking closure. Uses collection type
  `etebase.vtodo`; item.uid() → `RemoteTask::remote_id`.
  Content round-trips through `parse_vcalendar` +
  `vtodo_to_remote_task` so the existing VALARM / RRULE / parent
  handling applies uniformly.
- **CalDAV (Milestone 3).** `reqwest` 0.11 with rustls-tls.
  3-step discovery cascade (`PROPFIND /` → current-user-principal
  → calendar-home-set) runs inside `connect()`; results persist
  on a `Session` kept under `Arc<tokio::sync::Mutex<Option<…>>>`.
  PROPFIND and REPORT use `Method::from_bytes(b"…")` since reqwest
  doesn't ship them. Pushes PUT with `If-Match: "<etag>"`; 412 →
  `SyncError::Conflict`. Bearer auth honoured first (for
  Fastmail/iCloud when they grow OAuth) with Basic as the
  fallback. DELETE tolerates 404.
- **Google Tasks (Milestone 4a).** Tasks REST v1. OAuth2
  installed-app flow: `authorize()` binds a `LoopbackReceiver`,
  opens the auth URL via a caller-supplied closure, blocks on
  the redirect inside `spawn_blocking`, exchanges the code at
  `oauth2.googleapis.com/token`. `ensure_fresh()` refreshes the
  access token 60 s before expiry and writes the rotated tokens
  back to the `TokenStore`. `list_tasks` requests
  `showCompleted=true&showHidden=true` since Tasks.org surfaces
  both; tombstones (`deleted=true`) are dropped on pull so the
  engine's own tombstone pass handles them. Update uses PUT;
  create uses POST when `remote_id` is empty.
- **Microsoft To Do (Milestone 4b).** Microsoft Graph
  `/me/todo`. Same PKCE loopback shape as Google, different
  endpoints + pagination: OData `@odata.nextLink` (absolute URL,
  not a page token). Scopes: `Tasks.ReadWrite offline_access`.
  Update uses PATCH (PUT replaces the whole resource on Graph
  and would drop fields we don't serialise). etag lives on
  `@odata.etag`. 429 rate-limit responses bucketed as
  `SyncError::Network` so the engine retries on its next tick.

Testing: every provider ships offline unit tests for the
pure-logic bits (auth header selection, token response parsing,
HTTP error classification, path-segment encoding). Network paths
await real accounts + integration fixtures; we can't mint
credentials in CI. Workspace totals 182 tests after the
Microsoft commit, green under `cargo clippy --all-targets -D
warnings`.

Dependencies added with the real implementations:
`reqwest` 0.11 (rustls-tls + json), `etebase` 0.5, `tokio` 1 as
a real (not just dev) dep for `spawn_blocking` + async Mutex.
No `oauth2` crate — the PKCE + URL plumbing stayed in
`tasks_sync::oauth`, which is enough for all three OAuth
providers.

Remaining follow-ups before any provider is user-visible:

- **UI wiring.** The cxx-qt bridge needs an Accounts panel that
  lists configured providers, surfaces the `authorize()` call
  for OAuth ones, and exposes `SyncEngine::sync_now` to a "Sync
  now" toolbar button. Currently the sync layer compiles + tests
  but isn't reachable from QML.
- **OS-native token store.** `InMemoryTokenStore` is fine for
  tests but loses OAuth tokens on restart. Keyring adapters
  (libsecret / Keychain / Credential Manager) stay additive on
  top of the existing `TokenStore` trait.
- **Integration fixtures.** Radicale for CalDAV, a recorded
  Graph + Tasks API fixture via a tape recorder
  (rustic-tape / vcr-rs) for the REST providers, and an Etebase
  test-server container for the E2EE path.
- **Refresh-token leaks on ensure_fresh failure.** Currently a
  refresh-endpoint 4xx drops the session into an Auth error and
  the user has to reauthorise. That's correct but unfriendly;
  the UI should surface "reauthorise" as a first-class action.

## 12. Settings window lands; edit dialog becomes a real Window

Two UX pieces that fell out of first user testing of v0.0.8
landed together on this branch:

- **Tabbed Settings window.** The single "Preferences…" toolbar
  button that used to open a list-view-only Dialog is now a
  "Settings…" button that opens a top-level `ApplicationWindow`
  with a TabBar across `List` + `Accounts`. The previous
  preferences controls (sort mode + direction, show completed /
  hidden, completed-at-bottom) moved into the List tab
  unchanged; their Save button still calls
  `updatePreferences(...)` on the bridge. The Settings window
  itself is a real window with native title-bar move/resize/
  close; hide-on-close preserves the selected tab between opens.
- **Accounts pane (first pass).** New QML pane with an add/remove
  form over a parallel-array Q_PROPERTY surface on the bridge
  (`account_labels`, `account_kinds`, `account_servers`,
  `account_usernames`) backed by an in-memory `Vec<StoredAccount>`
  on the Rust side. CalDAV and EteSync entries accept
  label + server + username + password inline and land
  immediately; Google Tasks + Microsoft To Do appear in the
  provider picker but their "Sign in…" button is disabled with
  a tooltip pointing at §11's OAuth-flow follow-up. Passwords
  are held on the Rust side only and never cross the FFI
  boundary into QML.
- **Sync engine is not yet reachable.** Adding an account only
  records it; no Provider is constructed, no
  `SyncEngine::sync_now` is dispatched. The next commit pulls a
  tokio runtime onto the bridge and wires the "Sync now" action
  plus OAuth authorize() for the two browser-flow providers.
- **Edit dialog: scrollable, movable, resizable.** The previous
  `TaskEditDialog` was a `QtQuick.Controls.Dialog` with a pinned
  implicit size, which cut off the bottom of the form (Tags /
  Location / Reminders / Repeats) on compact displays and
  couldn't be moved or resized. The new version is a top-level
  `ApplicationWindow` with a ScrollView wrapping the full form,
  a manual Cancel / Save footer row, and native
  move/resize/close from the window manager. The notes field's
  inner ScrollView was removed so the outer viewport handles
  all scrolling (avoids nested-scroll mouse-wheel ambiguity);
  validation messages from the Rust-side parser stay pinned
  below the scroll so they're always visible after a failed
  save.

Remaining Settings/Accounts follow-ups (explicit):

- Persist accounts + preferences to `QSettings` (or an
  equivalent desktop-native config store) so neither the list
  of accounts nor the password + list preferences evaporate on
  restart. Separate keystore for the password itself
  (libsecret / Keychain / Credential Manager) already tracked
  in §11.
- Wire the Accounts pane's "Sync now" action per row (requires
  a tokio runtime on the bridge + a `Provider` factory that
  reads `StoredAccount` + credentials from the keystore).
- Swap the OAuth provider entries from "coming soon" to a real
  `authorize()` button once the browser-opener + loopback
  plumbing is reachable from QML.

## 13. Pending design decisions surfaced by the security + review passes

Five items deferred from the security pass (commit `a6e1172ab`)
and the five-axis review pass (commit `28f77f210`) need design
calls before they're implementable. Each is scoped here as a
mini-spec: problem statement, options, recommendation, and the
question the user has to answer.

### 13.1 F6 — silent in-memory credential degradation

**Problem.** When `KeychainTokenStore::probe()` succeeds at one
launch but `EncryptedFileTokenStore::probe()` fails at a later
launch (encrypted file's auth tag fails to verify, the file is
gone, machine-id rotated after a system re-image), `StorageTier`
silently falls through to `InMemorySecretStore` and just logs
at `warn!`. The QML disclosure dialog doesn't re-pop because
`credential_storage_acknowledged` is still `true` from the
prior session. The user sees their tokens "needed re-issuing"
without learning their persistent tier degraded — a phishing
opportunity if the next OAuth re-sign-in feels routine.

**Options.**

1. **Re-pop the disclosure on every probe-tier downgrade.** Detect
   "active tier ≠ user-requested tier (or last-recorded persistent
   tier)" at startup; if downgraded, clear
   `credential_storage_acknowledged` and surface a banner. Most
   conservative; flag-the-user-every-time.
2. **Re-pop only when the active tier landed on `InMemory`.**
   Narrower trigger. `Keychain → EncryptedFile` fallback
   wouldn't bother the user (still persistent), but
   `EncryptedFile → InMemory` would — that's the volatile
   case where re-sign-in *will* be required.
3. **Add a "credential storage demoted" status banner without
   re-popping the disclosure** — the user keeps using the app
   but sees the demotion in the Accounts pane.

**Recommendation:** option 2 + a small per-session "demoted"
flag the Accounts pane reads. Re-popping the modal disclosure
on every launch the keychain is briefly unavailable (e.g. on
GNOME login before the keyring unlocks) would be too noisy.

**Decision needed:** which trigger semantics. Once chosen,
implementation is ~30 lines in `bridge.rs::open_at_path` +
a per-row label binding in `AccountsPane.qml`.

### 13.2 F9 — OAuth re-sign-in throttling

**Problem.** Today `begin_oauth_sign_in` and `start_oauth` have
no minimum interval between attempts. A buggy QML signal
handler or a malicious clipboard auto-paste could fire the
flow in a tight loop, opening a system-browser tab on each
call and burning through PKCE / state pairs against the
authorisation server (which Google + Microsoft both rate-limit
on their end, surfacing as opaque 4xx responses).

**Options.**

1. **Per-account "earliest-next-attempt" timestamp.** Refuse
   to start a new flow if the previous one began less than N
   seconds ago. N = 5 s for first three attempts, exponential
   backoff after. Surface as a status-bar message
   ("Throttled: try again in {n}s") and an inert Sign-in
   button.
2. **Single in-flight guard.** Only one OAuth flow at a time
   regardless of which account. `oauth_stop` already exists
   for the cancellation path; gate `start_oauth` on
   `oauth_stop.is_none()`.
3. **Rely on the AS rate limit.** Don't add a client-side
   throttle; let the AS's 4xx surface. Simplest but exposes
   the user to the noise.

**Recommendation:** options 1 + 2 together (one in-flight
plus per-account cooldown). The cooldown numbers are
engineering, not policy — pick conservative defaults
(5 s / 15 s / 60 s exponential) and revisit only if a real
flow surfaces a friction point.

**Decision needed:** the cooldown numbers; the surface (toast
vs inline status); whether throttle on user-initiated re-
sign-in only or also on auto-sync re-auth attempts.

### 13.3 Rustls upgrade — `reqwest 0.11` → `0.12`

**Problem.** `cargo audit` flags three CVEs in `rustls-webpki
0.101.7` (RUSTSEC-2026-0098 / -0099 / -0104): two name-
constraint bypasses on cert validation, one reachable panic
in CRL parsing. We pin `rustls-tls` explicitly in both
`tasks-sync` and `tasks-ui` Cargo.toml entries, so the
advisory applies to every CalDAV / Google / Microsoft / Etebase
request. The CVE fix path is `rustls-webpki ≥0.103.13`, which
requires `rustls ≥0.23`, which means `reqwest 0.12`.

**The migration cost is non-trivial.** `reqwest 0.12`
changes:

- Builder ergonomics: `redirect::Policy::none()` and
  per-request `header()` calls are stable, but several
  `Client` setters renamed.
- Default features: drops `default-tls`; we already opt in
  with `rustls-tls`, so no breakage there.
- Stream consumption: `Response::bytes_stream()` API changed
  shape — our `read_body_capped` consumer needs a 5-line
  update.
- TLS error types: a couple of variants moved between
  `reqwest::Error` sources.

**Options.**

1. **Bump reqwest to 0.12 across the workspace.** One
   commit; touches every provider + `bridge.rs::http_for_oauth`.
   Run the whole test suite + manual smoke against a real
   CalDAV/Etebase to flush out behavioural drift.
2. **Pin `rustls-webpki` via `[patch.crates-io]`.** Force the
   transitive dep; reqwest 0.11 will keep using its old API
   on the rest of the rustls stack. May or may not work
   depending on rustls 0.21 ↔ rustls-webpki 0.103
   compatibility (likely a semver hazard).
3. **Switch from `rustls-tls` to `native-tls`.** Drops the
   advisory entirely (uses OpenSSL on Linux, SChannel on
   Windows, Secure Transport on macOS). Behavioural
   differences across platforms become our problem.
4. **Accept the risk + document.** The advisories are
   pre-conditional (require attacker control of cert chain,
   already MITM-able is a higher bar for our user-supplied
   server URLs). Track the upgrade for a follow-up
   milestone.

**Recommendation:** option 1, but on its own branch
(`claude/rustls-upgrade`) so the merge to main can revert
cleanly if a manual smoke surfaces drift. Estimate: 200-400
lines touched, ~3 hours of human dev time after the patch.

**Decision needed:** which option; if 1, when (likely
post-GUI-QA so the upgrade doesn't invalidate manual
testing).

### 13.4 Test-architecture gaps — F1 + F4

Two regression-test gaps the five-axis review surfaced. Pairing
them because both need a small piece of test infrastructure
the codebase doesn't yet have.

**F1 — CalDAV `connect()` ordering not directly tested.** The
load-bearing property of the CRITICAL fix in
`caldav.rs::connect()` is "given a server that returns an
absolute cross-host `<d:href>`, the second authenticated
PROPFIND must NOT be issued, and connect() must return
`SyncError::Protocol`." `trusted_origin_rejects_off_origin_targets`
tests `TrustedOrigin::check` in isolation but doesn't drive
the connect() flow.

**F4 — `update_password_account` rollback not unit-tested.**
The "put_secret OK, DB UPDATE fails → delete_secret rollback"
branch is exercised only by code review. Forcing it from
outside requires either a closed/read-only DB connection or
an injectable DB writer the test can fail.

**Options for F1 (HTTP-mocking).**

1. **Add `wiremock` as a dev-dep.** Standard Rust HTTP-mock
   crate; lets us write `Mock::given(method("PROPFIND"))
   .respond_with(ResponseTemplate::new(207)
   .set_body_xml(absolute_href_response()))`. ~15 KB of
   compiled test code.
2. **Hand-roll a `tokio::net::TcpListener`-based responder
   in test.** Smaller dep cost; messier test code. CalDAV's
   PROPFIND verb makes this awkward (reqwest wraps the
   custom-method support; we'd be hand-coding HTTP/1.1).
3. **Ship without the test.** F1's mechanism is
   straightforwardly verifiable by code reading; the cost
   of a regression hides in a future change re-ordering
   the four lines that establish + check trusted_origin.
   Documented gap.

**Options for F4 (DB-writer injection).**

1. **Extract a `WriteAccountFn` trait the test can implement.**
   Replaces the inline `open_rw_conn(&path).and_then(...)` with
   a callable. Adds one type to the bridge module. ~20 lines.
2. **Point `db_path` at a read-only file in the test.** Forces
   `open_rw_conn` to error before the UPDATE runs — exercises
   the rollback path without a refactor. Limitation: doesn't
   cover the "open succeeds, UPDATE fails" branch (which is
   what the rollback was originally designed for); tests the
   whole-failure case instead.
3. **Ship without the test.** Same trade-off as F1: the fix
   mechanism is reviewable but a future regression in the
   rollback ordering would be invisible to CI.

**Recommendation:** for F1, option 1 (wiremock). For F4,
option 2 (read-only DB file) — the broader fix mechanism
catches what we actually care about (no leaked secret on
DB failure of any shape). Combined cost: ~80 lines of test
code, one new dev-dep.

**Decision needed:** whether the wiremock dep cost is
worth the pin on F1 ordering. If yes, this lands in one
follow-up commit alongside the F4 read-only-DB test.

### 13.5 User-supplied master password (currently reserved-only)

**Problem.** `EncryptedFileTokenStore` derives its AES-256-GCM
key from `HKDF-SHA256(machine-id, per-file-salt)`. Machine-id
is readable on every supported platform by any local process
running as the user (Linux `/etc/machine-id` is world-readable;
macOS IORegistry UUID and Windows scheduler UUID are similarly
exposed). The encrypted-file tier therefore protects against
`tasks.db` / `oauth.bin` exfiltrated *without* the rest of the
machine (cloud-backup leak, USB-stick theft) — but does NOT
protect against another local user-mode process reading both
the file AND the machine-id and decrypting locally.

A user-supplied master password closes that gap: the cipher
key becomes `Argon2id(passphrase, per-file-salt)`, machine-id
no longer enters the derivation, and the passphrase lives only
in process memory (SecretString) for the lifetime of the run.

**Status today.** Slot reserved, nothing wired:

  * `KdfMarker::MasterPassword = 1` is in the file-format
    enum and the byte-table layout.
  * `decode_file` reads the marker and returns a clear
    "master-password mode is not supported in this build"
    error.
  * `decode_rejects_master_password_until_supported` pins
    the forward-compat rejection.

What's missing: the Argon2id derivation (parameter selection
— memory cost / time cost / parallelism), a passphrase-prompt
QML dialog at startup when the file uses the MasterPassword
marker, a "set / change / remove master password" flow in
Settings, a migration path that re-encrypts the file when the
user opts in or out, and the in-memory passphrase lifecycle
(prompted re-entry on session lock, cleared on unlock).

**Options.**

1. **Argon2id with conservative params + always-prompt at
   startup.** Library: `argon2` crate. Parameters: 64 MiB / 3
   iter / 1 lane (OWASP 2024 floor). Prompt blocks app launch
   until the user enters the passphrase; mistype gets re-
   prompted (with rate-limit). On enable / disable, re-encrypt
   the file once. ~400 LOC + a new QML dialog.
2. **Optional master password — opt-in via Settings.** Same
   as 1 but unmarked installs continue using machine-id. The
   user ticks "use a master password" and provides one; the
   file is re-encrypted with MasterPassword marker. Removing
   it re-encrypts back to MachineId. More UX surface; same
   crypto.
3. **Defer.** The threat model gap is real but requires same-
   machine attacker access, which most desktop users don't
   defend against in any other layer either.

**Recommendation:** option 2 once Milestone 1 / 2 GUI testing
is settled. The forward-compat slot was deliberately reserved
for this; abandoning it would be a soft regression. Engineering
effort is comparable to one of the M2 sub-features.

**Decision needed:** whether to land this before M3 (CalDAV
write path) or after; the param-selection details (Argon2id is
non-trivial to size — too aggressive locks out laptops on
battery, too weak makes the password meaningless).
