# Test fixtures

`build_fixture_db` in `tests/db_smoke.rs` synthesises a minimal SQLite
database matching Room schema version 92 at runtime, so no binary fixtures
are checked in. When porting the full query layer from `kmp/`, captured
DBs exported from the Android instrumentation tests will land here.
