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

## 7. Bottom line

The original plan survives largely intact. The four material
updates are:

1. The parallel-Q_PROPERTY model shape replaces the
   `QAbstractListModel` goalpost for Milestone 1 and promotes it to
   Milestone 2 instead.
2. Rust MSRV pinned at 1.82.
3. Non-recursive query path was moved forward from "later milestone"
   into Milestone 1 without cost.
4. **Query-builder parity testing must be comprehensive up front.**
   The original plan treated *"port and then spot-check"* as
   adequate; the code review proved otherwise. Going forward, every
   query-level constant gets a paired ASC/DESC test, and every
   predicate-rewrite helper (`show_hidden`, `show_completed`, future
   `removeOrder`) gets an explicit before/after fixture.

No milestone target has slipped; the dependency risks (iCloud CalDAV
quirks, QtWebEngine bloat, schema drift, `libetebase` drift) called
out in the original `## Risks` section remain accurate.
