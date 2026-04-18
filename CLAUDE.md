# Repository guidance for Claude

## Layout

- `app/` — Android application (Kotlin + Compose)
- `kmp/` — Kotlin Multiplatform module (targets `androidTarget()` and `jvm()`; currently shares query builders and a handful of Compose UI components)
- `data/` — Kotlin Multiplatform data module (Room entities, DAOs, schemas). Canonical schema JSON lives in `data/schemas/org.tasks.data.db.Database/<version>.json`.
- `wear/` and `wear-datalayer/` — Wear OS companion
- `desktop-native/` — **Native (Rust + Qt 6) desktop client.** See `desktop-native/README.md`. Separate from any Kotlin/JVM desktop experiment.

## Ignore

The `desktop` branch on origin (to be renamed `jetpack-desktop`) contains a
partially-built **Compose-for-Desktop (JVM/Kotlin)** client under a top-level
`desktop/` directory. That effort is deprecated in favour of the Rust + Qt 6
native client under `desktop-native/`. When working on the native desktop
client, do **not** read or port from the `jetpack-desktop` branch — it uses a
different language, runtime, and UI toolkit, and its sync code depends on JVM
libraries (dav4jvm, etebase JVM bindings, OkHttp) that aren't the target for
the native client.

If you see references to a top-level `desktop/` directory, confirm the branch
before using it as context. On `main` the directory for the native client is
`desktop-native/`, not `desktop/`.
