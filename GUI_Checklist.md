# Manual GUI Test Checklist — claude/native-desktop-client-5FFnd

| | |
|---|---|
| Date | 2026-05-09 |
| Tester | Christopher Shiflet |
| Build commit | 3ab232837 |
| Platform | Linux |
| Run command | cargo run -p tasks-ui |
| Log level | RUST_LOG=info (recommended) |

Legend: PASS / FAIL / PARTIAL / SKIP

---

## 0. Pre-flight setup

- [x] Built clean: cargo build --workspace succeeds.
- [x] Have at least 2 sync accounts configured (preferably 1 CalDAV + 1 OAuth, or 2 CalDAV).
- [x] Have a fixture DB with at least 5–10 tasks across multiple lists.
- [x] Tail RUST_LOG=info output in a separate terminal so you can spot warnings as they happen.

Notes / setup issues:
(write notes here)

---

## 1. Sync-feedback UI (the new green checkmark overlay)

| # | Test | Result | Notes |
|---|---|---|---|
| 1.1 | Click Sync icon on one account → green check overlays the refresh icon for ~3 s, then disappears cleanly. | pass | |
| 1.2 | Click Sync twice in quick succession (second click within 3 s of the first completing) → check disappears immediately when the second sync starts. It must NOT overlay the spinning refresh glyph (A-R3-4 fix). | pass | |
| 1.3 | Trigger a sync that fails (point at an unreachable server, or kill network mid-sync) → red X overlay shows; no green check. | fail | The timeout is extremely long. The button never showed the red x overlay. |
| 1.4 | After a re-sign-in, the next successful sync shows green check and the X is gone. | | Blocked by 1.3 |
| 1.5 | Auto-sync (timer-driven) shows the same green check on success without user interaction. | | I don't know what the timer duration is. Need to be able to configure that in settings. Then I can retest. |

Failure details:
(write notes here)

---

## 2. Parallel-sync database concurrency (the original "database is locked")

| # | Test | Result | Notes |
|---|---|---|---|
| 2.1 | With 2+ accounts configured, click "Sync all" (or whatever fans out) → both accounts complete without "database is locked" anywhere. | pass | Fixed in this session — see Failure details. Verified with 4 accounts (caldav, etesync, gtasks, mstodo) over 2 sync rounds; all 8 pulls completed, including mstodo's 3 calendars / 842 tasks which had been failing on every prior startup. |
| 2.2 | Tail of RUST_LOG=info: no SQLITE_BUSY warnings during the parallel sync. | pass | Originally 3 "database is locked" events per startup. Fix verified: 0 warnings across 2 full sync rounds. |
| 2.3 | Click "Sync all" three times in rapid succession → each sync either runs or is silently absorbed (re-entrancy guard); no panics. | pass | |
| 2.4 | Verify journal_mode in the DB: open sqlite3 <db> "PRAGMA journal_mode" after first sync → reports wal. | pass | Verified via python3 sqlite3 (sqlite3 CLI not installed): journal_mode = wal, busy_timeout = 5000. |

Failure details (resolved):

Two root causes, both fixed in this session:

1. **HTTP I/O held inside the write tx.** `pull_all` (engine.rs) used to open `BEGIN DEFERRED`, then call `provider.list_tasks(...)` *while still holding the tx*. With multiple parallel accounts, account A's pull held the write lock during HTTP, and account B's first write inside its own DEFERRED tx hit SQLITE_BUSY before the 5 s busy_timeout could rescue it.
   - **Fix:** Stage every calendar + its task list in memory via HTTP *before* opening the write tx. The tx now only spans the SQLite work, not network round trips.
2. **`BEGIN DEFERRED` upgrade contention.** All production `.transaction()` calls used rusqlite's default DEFERRED. With multiple writers, the second to arrive can hit SQLITE_BUSY on its first INSERT in a way busy_timeout doesn't reliably retry.
   - **Fix:** Switched all 4 production `.transaction()` sites (engine.rs::pull_all, write.rs::create_task, write.rs::edit, bridge.rs::delete_account_cascade, bridge.rs::vacuum_orphan_tasks) to `transaction_with_behavior(TransactionBehavior::Immediate)`. Write lock is acquired at BEGIN time, where busy_timeout retry is reliable.

---

## 3. Sync status-line text formatting

| # | Test | Result | Notes |
|---|---|---|---|
| 3.1 | Sync that pulls but pushes nothing: badge reads "Synced (N down / 0 up)". | pass | Settings→Accounts is the only surface for the badge text; tester confirms that's acceptable. Format matches: `Synced (N↓ / 0↑)`. **Caveat:** see 3.4 — the N count is buggy (always = total tasks server returned, not delta from last sync). |
| 3.2 | Sync that pushes but pulls nothing: "Synced (0 down / M up)". | partial | Edits trigger an auto-sync via `auto_sync_for_task` (bridge.rs:2601), so by the time the tester checks Settings→Accounts the badge has already cycled. Badge format is correct (verified via code inspection: `"Synced ({}↓ / {}↑)"` at bridge.rs:2351). Re-test by adding tracing to the spawned-sync completion site, OR by editing immediately before clicking Sync to capture the moment. |
| 3.3 | Sync that propagated soft-deletes upstream: "Synced (N down / M up / K trash)" (the trash count only appears when K >= 1). | pass | Initially partial (correct count, but 🗑 emoji glyph rendered as tofu in Qt's bundled font). Fixed in this session: format string in bridge.rs swapped to ASCII ` trash` suffix. Verified: badge now reads `… / 1 trash` with no broken glyph. |
| 3.4 | Pure no-op sync: "Synced (0 down / 0 up)", no trash suffix. | pass | Initially fail (counts always showed total tasks server returned, e.g., MS To Do stuck at 842). Fixed in this session: `upsert_task` now returns `UpsertOutcome { changed: bool }` and `pull_all` only counts rows whose stored `modified` was actually advanced by the new pull. Verified: all accounts now drop to `0↓ / 0↑` on a no-op sync. Regression test `second_pull_with_no_remote_changes_reports_zero_pulled` added. |
| 3.5 | Sync conflict (manufacture a server-side edit between local edit and push): badge mentions conflicts; no data loss. | skip | Deferred — manufacturing a conflict requires external tooling (Android client, CalDAV web UI, or curl against the endpoint). Per tester: revisit much later in test plan. |

Failure details:

§3.4 — `tasks_pulled` count semantics. The engine's `pull_all` (engine.rs:124) does `tasks_pulled += 1` for every task in `provider.list_tasks(...)`, treating "server returned this task" as equivalent to "we pulled it down". For providers that always list every task (rather than returning only changed items via etag/sync-token), the count never decreases below the total. Two ways to fix:

1. **Engine-side:** `upsert_task` (engine.rs:657) currently runs an unconditional UPDATE for matched rows. Add a no-op detection — either tighten the WHERE clause to `... AND modified < ?chosen_modified` (then check `tx.changes()` for 0), or query the existing row's `modified` first and skip the UPDATE when it matches. Return the no-op signal up to `pull_all` and only increment `tasks_pulled` for rows that actually wrote. ~30 lines including a small test.
2. **Provider-side:** add an etag/sync-token round trip to `list_tasks` so the provider returns only changed items. Bigger lift, varies per provider; orthogonal to the badge issue (still want #1 even with sync-tokens since the engine is the source of truth on "did anything change locally").

Recommend #1 for the badge fix, with #2 as a separate efficiency optimization later.

**Bonus glyph bug from §3.3:** the format string at bridge.rs:2344 uses `🗑` (U+1F5D1 WASTEBASKET) which renders as a tofu box in this Qt env. Either swap to a Material Symbols icon (other icons in the app use that family) or fall back to ASCII (`del`, `[del]`).

---

## 4. Credential storage tier switching

| # | Test | Result | Notes |
|---|---|---|---|
| 4.1 | Settings → switch tier from Auto → In-memory → restart the app → password-auth account re-prompts for credentials. | | |
| 4.2 | Switch In-memory → Auto → password is migrated and survives restart. | | |
| 4.3 | Switch to the same tier you're already on → no log churn, no preferences.json mtime change (B-R3-6 no-op). | | |
| 4.4 | On Linux: switch to Keychain tier with gnome-keyring not running → falls back gracefully to encrypted-file tier; status reflects the active tier. | | |
| 4.5 | After switching tiers, verify tasks.db caldav_accounts.cda_password is blank (sqlite3 <db> "SELECT cda_password FROM caldav_accounts") and the password is in the new store. | | |

Failure details:
(write notes here)

---

## 5. Soft-delete propagation

| # | Test | Result | Notes |
|---|---|---|---|
| 5.1 | Soft-delete a synced task → next sync of that account propagates the delete; status shows "/1 trash"; task is gone server-side. | | |
| 5.2 | Soft-delete a brand-new local-only task (never synced) → no server call (verify via account log); local row removed; no trash count. | | |
| 5.3 | Delete a task while offline → local row gets soft-deleted; on reconnect + next sync, the delete propagates. | | |
| 5.4 | Concurrent edit conflict on delete (manufacture a server-side edit between your local soft-delete and the next sync): conflict surfaces in the status / log; row stays in "pending delete" state for next cycle. | | |

Failure details:
(write notes here)

---

## 6. Re-entrancy guard (A-R3-3 leak fix)

| # | Test | Result | Notes |
|---|---|---|---|
| 6.1 | Spam-click one account's Sync icon while a sync is in flight → only one sync runs; subsequent clicks silently absorbed. | | |
| 6.2 | After a finished sync, the icon is clickable again (slot was released). | | |
| 6.3 | Force a thread-spawn failure (low-resource / rapid spam) → status shows "Failed: ..."; sync slot released so the next click works. (Hard to reliably reproduce; OK to skip.) | | |

Failure details:
(write notes here)

---

## 7. OAuth account flows (Google / Microsoft)

| # | Test | Result | Notes |
|---|---|---|---|
| 7.1 | Add a fresh Google Tasks account → browser opens, complete sign-in → returns cleanly to app; first sync succeeds. | | |
| 7.2 | Add a fresh Microsoft To Do account → same shape. | | |
| 7.3 | Close the app while OAuth window is open → no zombie loopback receiver (next launch's OAuth flow works fine). | | |
| 7.4 | Sign in once, restart app → token persisted (no browser re-prompt for the next sync). | | |
| 7.5 | Manually expire the token (edit DB / wait it out) → status shows "Re-sign-in required"; clicking Re-sign in triggers the browser flow without crashing. | | |

Failure details:
(write notes here)

---

## 8. General CRUD / regression sanity

| # | Test | Result | Notes |
|---|---|---|---|
| 8.1 | Create a task on a CalDAV list → propagates on sync. | | |
| 8.2 | Edit a task's title / due / priority → propagates. | | |
| 8.3 | Mark a task complete → propagates. | | |
| 8.4 | Built-in filters (Today, Snoozed, etc.) populate correctly. | | |
| 8.5 | Theme follows OS dark/light toggle (toggle while app is running). | | |
| 8.6 | Subtasks render with correct indent; collapsing a parent hides children. | | |
| 8.7 | Tags / places / custom filters all reachable from the sidebar. | | |

Failure details:
(write notes here)

---

## 9. Misc observations

Things you noticed that don't fit a checkbox above (UX rough edges, perf concerns, log noise):

- **Tokio panic in sync:mstodo (resolved).** During §2 testing, mstodo's sync thread panicked with "A Tokio 1.x context was found, but it is being shutdown." Investigation showed it was a downstream consequence of the database-locked failure cascading into runtime teardown. Resolved by the §2 fix — re-verified with 0 panics across 2 full sync rounds.
- **Hover flicker + click-doesn't-select in the middle pane (resolved).** Hovering a task row caused the highlight to blink rapidly; clicking a row never populated the right pane. Root cause: the filesystem watcher fired ~10 Hz on its own activity (in WAL mode, every read touches `-shm`, which inotify sees as a modification → debounce → reload → another read → loop). Each reload's `set_count(0) → set_count(N)` cycle in `publish_tasks` destroyed and recreated every QML delegate, killing both hover state and any in-flight press. Resolved with three layered fixes:
  1. Watcher callback now gates on `PRAGMA data_version` and skips reload if no commit has actually happened (breaks the self-induced loop at the source).
  2. `publish_tasks` early-returns when the new task list is byte-identical to the cached one (belt-and-braces against any other reload trigger).
  3. `task_cache` assignment moved before `set_count(N)` in `publish_tasks` so a click landing on a freshly-published delegate finds its task in the cache instead of silently dropping into `clear_detail_pane`.
- **TaskListPane click handler swap.** `ItemDelegate.onClicked` was being silently swallowed during the rapid delegate destruction (press fired, release went to a destroyed delegate, no `clicked` signal). Switched to `TapHandler` matching SidebarPane's pattern. With the underlying churn fixed, `onClicked` would also work, but `TapHandler` is kept as defense-in-depth + consistency.
- **Watcher debounce 250 ms → 100 ms.** Lower latency for external writes (Syncthing, Android via shared file). Now safe because the data_version gate prevents amplification.
- **No `sqlite3` CLI in this dev env.** Used `python3 -c "import sqlite3; …"` for §2.4 verification. Not a bug, just a workflow note.

---

## 10. Summary

- Total tests run: __ / 41
- Passes: __
- Failures: __
- Critical issues to flag back to Claude:
(write notes here)
