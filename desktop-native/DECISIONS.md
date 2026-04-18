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
