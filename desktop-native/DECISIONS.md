# Decision log — `desktop-native/`

This document records non-obvious choices made while implementing the
Rust + Qt 6 native desktop client. Each entry cites the alternatives
considered and the constraint that decided it, so future contributors
don't have to re-derive the context.

## 1. Language + toolkit: **Rust + Qt 6 via cxx-qt 0.7**

**Chosen** over C++ / Qt and Go + Qt because:

- **Library parity with upstream Kotlin.** Rust's sum types, `Option`,
  pattern matching, and iterator chains port from Kotlin idioms more
  faithfully than Go or C++.
- **`libetebase` is Rust-native.** The Etebase client (needed for the
  EteSync sync backend in a later milestone) is itself written in Rust;
  linking it from Rust avoids an FFI boundary we'd otherwise own.
- **Memory safety at the boundary.** The bridge code touches raw pinned
  pointers (`Pin<&mut Self>`) — Rust's borrow checker catches aliasing
  mistakes that would be silent UB in C++.

**Trade-off accepted:** `cxx-qt`'s community is smaller than the C++/Qt
community, and some Qt types (notably `QAbstractListModel` with a
custom roleNames() mapping) need hand-rolled C++ glue rather than being
built in. We accept this in exchange for the Rust-side ergonomics.

## 2. Model shape: **parallel Q_PROPERTYs, not QAbstractListModel**

The natural way to expose a task list to QML would be subclassing
`QAbstractListModel` and exposing per-row roles via `roleNames()`.
`cxx-qt` 0.7 supports the `#[base = QAbstractListModel]` attribute, but
the required `rowCount`, `data`, and `roleNames` overrides need to be
written in C++ and bridged through cxx, adding roughly 150 lines of
scaffolding for a read-only model.

Instead the bridge exposes **parallel Q_PROPERTYs** (`titles`,
`taskIds`, `indents`, `completedFlags`, `dueLabels`, `priorities`) that
QML delegates index by row number:

```qml
ListView {
    model: viewModel.count
    delegate: ItemDelegate {
        text: viewModel.titles[index]
        ...
    }
}
```

**Why this is acceptable:**

- Read-only client; no per-row mutation signals needed.
- File-watch reloads the whole DB anyway; a full-replace model is the
  natural cadence.
- Removing ~150 lines of C++ FFI plumbing reduces the surface area
  someone else has to understand.

**When to revisit:** Once the write path or CalDAV sync lands,
fine-grained row updates (e.g. "task 42 just got marked complete")
become worth the beginInsertRows/endRemoveRows machinery. At that
point, promote the view model to a real QAbstractListModel.

### 2a. QVariantMap/QVariantList was considered and rejected

An intermediate design used `QList<QVariant>` where each variant wraps
a `QMap<QString, QVariant>` — each task a dict, QML iterates natively
with `modelData.title` etc. Rejected because `cxx-qt-lib` 0.7 ships
conversions from primitive types and a handful of Qt classes to
`QVariant`, but **not** a `QVariant::from(&QMap<QString, QVariant>)`
path. Adding the conversion requires a custom `cxx::bridge` block with
C++ template specialisation — more code than the parallel-property
shape above and with no visible benefit for this milestone.

## 3. Custom filters: route through prepared statements, not string
   concatenation.

The sidebar's `filter:<row_id>` entries load the filter's SQL directly
from the `filters.sql` column and hand it to `build_query`, which then
runs through `prepare()`. That protects us from two classes of error:

- **SQL syntax issues** from user-authored filters surface as a
  `SQLITE_ERROR` at prepare time, not silent corruption of the
  resulting view.
- **Injection via untrusted filter rows** — even though the `filters`
  table is written by the Android app (not by the desktop client), we
  treat its SQL as data and never concatenate it unchecked. The uuid
  in `caldav_parent_query` is single-quote-escaped for the same reason.

## 4. Schema pinning: **hard-fail on drift, don't auto-migrate**

`tasks_core::db::PINNED_IDENTITY_HASH` is a constant, not a best-effort
match. Opening a DB with a different hash returns `SchemaMismatch`
immediately, and a CI job diff-checks the pinned constant against
`data/schemas/org.tasks.data.db.Database/<V>.json` on every PR that
touches either file.

**Alternative rejected:** maintain compatibility shims per-column.
Rejected because the desktop client is read-only and Room's identity
hash is a strong signal that *something* in the schema changed, which
we want a human to triage rather than silently paper over.

## 5. Date math: **port Howard Hinnant's algorithm, don't pull in
   `chrono` or `time`**

`format_due_label` currently returns UTC ISO-like strings
(`YYYY-MM-DD`, optionally `HH:MM`). Bringing in a full date/time crate
would give us timezone-aware formatting but also adds a transitive
dependency graph we don't need yet.

The local implementation uses the well-known Howard Hinnant
`civil_from_days` algorithm (era offset 719468), verified against
known dates (1970-01-01, 2000-01-01, 2020-02-29 leap day). When we
start displaying due dates in the user's local timezone with
locale-aware weekdays, swap in `time 0.3` and delete `days_to_ymd`.

## 6. "Has time" detection: **match Android's seconds-flag convention**

Android's `Task.hasDueTime(dueDate)` returns true iff
`dueDate / 1000 % 60 > 0`. The app stores date-only tasks at midnight
(all components zero) and timed tasks with a **non-zero seconds
component** — this lets a single `long` encode both flavours without a
separate column.

`format_due_label` matches the same rule, which is why the test uses
`2020-02-29 12:34:01` (one second past the minute) rather than
`12:34:00` to represent a timed task.

## 7. Offscreen Qt in CI: **`QT_QPA_PLATFORM=offscreen`, no xvfb**

CI runs the Qt binary headless by setting `QT_QPA_PLATFORM=offscreen`.
No X server, no xvfb dependency, no display secrets. We still get full
QML parse + object graph construction, which catches the bulk of
integration-layer bugs.

**Rejected alternatives:**
- Launch under xvfb-run — works, but adds a process to manage and a
  display-server dependency to install.
- Skip GUI smoke entirely — leaves QML import errors uncaught.

## 8. Date handling in `format_due_label`: **UTC for now, local in
   follow-up**

For Milestone 1 the displayed date is UTC, matching how the database
stores it. A dedicated `time`-crate-backed formatter will land when the
UI grows timezone-aware components (reminder scheduling, today filter
across DST). Docs: see comment in `bridge.rs::format_due_label`.

## 9. Sidebar identifier scheme: **tagged string prefixes**

Sidebar rows pair `(label, id)` strings, with `id` using a small
typed-string grammar:

| Prefix         | Shape                        | Example          |
|----------------|------------------------------|------------------|
| built-in       | `__<name>__`                 | `__today__`      |
| caldav         | `caldav:<uuid>`              | `caldav:abc-123` |
| custom filter  | `filter:<row_id>`            | `filter:7`       |

The id round-trips through `selectFilter` unchanged; the dispatcher in
`tasks-core::query::run_by_filter_id` splits on the prefix and branches
to the right executor. A richer variant would expose a Rust enum to
QML, but that requires cxx-qt custom type registration — a tagged
string is a trivial compromise with zero new FFI surface.

## 10. Recursive query caldav uuid escaping

`caldav_parent_query` inlines the calendar uuid inside a single-quoted
SQL literal rather than binding it as a parameter. `build_recursive_query`
returns a SQL *string*, so binding parameters would require threading
bindings through both query builders and every caller. The escaping
(`'o\'brien'` → `'o''brien'`) is covered by a unit test. A future
refactor can parameterise once we have a proper query executor struct
that owns both the SQL string and the bindings.

## 11. MSRV: **bumped to 1.82** for cxx-qt

`cxx-qt 0.7.3`'s generated code uses APIs stable since Rust 1.82
(notably `Box::pin` sugar in position bodies). We bumped the workspace
`rust-version` rather than pinning an older cxx-qt or working around
the macro output. Rust 1.82 has been stable since late 2024.

## 12. **Desktop SQLite is exclusive-access, not shared with Android**

The original plan treated the Android-written DB as a file the desktop
could open over Syncthing / iCloud Drive / etc. Verified against the
Android app: that's not how the product works.

- `ProductionModule.kt` stores the DB at
  `context.getDatabasePath(Database.NAME)` — an
  `/data/data/<pkg>/databases/*` path that lives in Android's per-app
  sandbox and is not reachable by other apps or cloud-sync tools
  without root.
- `TasksJsonExporter.kt` is the user-facing backup path; it emits
  JSON, not a SQLite copy.
- Cross-device state is synchronised via CalDAV / Google Tasks / MS
  To Do / EteSync over the wire.

So the desktop client owns its SQLite file exclusively — no
Android-process writes it concurrently. `Database::open_read_only`
therefore holds only a short (50 ms) `busy_timeout` as a safety net
against a second desktop process, and the plan's Risk #3 ("SQLite
concurrency when Android is writing while desktop reads") is struck
entirely. See `PLAN_UPDATES.md` §6.6 for the knock-on effects on
Milestone 1 framing (a JSON-import path or CalDAV sync must
eventually seed the file; "read-only companion against a live
Android DB" was never a realistic UX).

## 13. Sync crate: async-trait + `Box<dyn Provider>`

**Chosen** over separate provider types + generics because:

- **Four backends, one UI.** CalDAV, Google Tasks, Microsoft To Do,
  and EteSync all need to co-exist on one desktop install. The UI
  holds `Vec<Box<dyn Provider>>` indexed by account and never sees
  concrete provider types, so a user can connect all four without
  cross-crate type changes.
- **`async-trait` over associated-type futures.** Keeps the trait
  object-safe (`dyn Provider`) and avoids pinning every call site
  on a specific `impl Future<Output = …>`. The desugaring cost is
  one heap allocation per method call — negligible against
  network round-trip latency.
- **No tokio-style runtime pinned at the trait boundary.** Each
  provider picks its own executor: CalDAV / Google / MS want a
  real async runtime (`reqwest`'s), EteSync wraps blocking FFI
  via `spawn_blocking`. The trait surface doesn't care.

## 14. Sync crate: skeletons first, network second

**Chosen** over shipping each provider as one big commit:

- The provider shapes — `AccountCredentials`, `RemoteCalendar`,
  `RemoteTask`, `SyncError` — are the hard part. Landing the
  trait + every stub + a `MockProvider`-driven orchestrator
  up front lets the UI and the merge logic evolve without
  waiting for HTTP / auth / encryption plumbing.
- Every stub returns `SyncError::NotYetImplemented` from its
  network method. Callers fail loudly instead of silently
  returning empty data — exactly the signal the UI needs to
  surface "this provider isn't wired yet" to the user.
- Per-provider dependencies (`reqwest`, `oauth2`, `libetebase-rs`)
  stay out of `Cargo.toml` until each backend's first real
  network commit. Keeps the skeleton build fast and the
  dependency surface area honest.

## 15. iCal handling: hand-rolled parser, not `icalendar` crate

**Chosen** over the third-party `icalendar` crate:

- **Scope control.** Tasks.org's wire use of RFC 5545 is a small
  subset (VTODO + a handful of properties + VALARM). Writing that
  as ~350 lines of pull-parsed Rust is cheaper than auditing the
  icalendar crate's 10k+ line surface for our specific
  round-trip-stability needs.
- **Lossless round-trip.** The desktop writes VTODOs back to
  CalDAV; any property we parse must serialise byte-for-byte
  similar enough that Android-side diffs aren't a constant noise
  source. A hand-rolled parser + serializer in the same file
  makes that invariant auditable.
- **No heavy transitive deps.** The iCal crate tree pulls in
  date/time libraries we don't need.

Scope trade-off: advanced RFC 5545 features (VTIMEZONE, EXDATE,
RDATE, RECURRENCE-ID) are not supported yet and will be added
directly to this module as they're needed.

## 16. OAuth2: PKCE by hand, skip the `oauth2` crate

**Chosen** over the `oauth2` crate:

- **Forced-runtime avoidance.** `oauth2` defaults to `reqwest`
  with its own tokio configuration; we want to pick the HTTP
  client and runtime ourselves (rustls-tls, custom user agent).
- **Crypto primitives we already need.** `sha2` + `base64` +
  `getrandom` are standard Rust deps each weighing ~50 KB of
  binary. The `oauth2` crate's dep tree is several megabytes.
- **Pure-logic + loopback + token-store separate.** The three
  concerns split naturally into three modules we can test
  independently (`oauth`, `loopback`, `token_store`).

## 17. Sync engine: "remote wins" pull merge, etag-gated push

**Chosen** over three-way merges:

- **Server as source of truth.** CalDAV / Google / Microsoft each
  have their own conflict-resolution model; rather than
  reimplementing, let the server win on pull and push only when
  the local etag still matches (412 Precondition Failed surfaces
  as `SyncError::Conflict` back to the UI).
- **Local-only state is non-authoritative per field.** Pull never
  overwrites `tags`, `alarms`, or `geofences` because the
  provider doesn't speak those tables; the local edit dialog is
  the sole writer for them. When the provider eventually does
  (Microsoft Graph has categories + reminders), the engine grows
  a per-field replace helper that respects the same "server
  wins if set, else preserve local" policy.
- **Deletes propagate on pull.** A task that was in the previous
  listing but isn't in the current one is soft-deleted locally —
  matches the Android client and gives the UI's trash filter
  something to surface.
