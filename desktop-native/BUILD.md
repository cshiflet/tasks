# Building `desktop-native`

Every path below lands you on the same dev loop:

```sh
cargo build --workspace
cargo test  --workspace
cargo run   -p tasks-ui                                # GUI
cargo run   -p tasks-ui -- --cli /path/to/tasks.db     # data-layer only
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Pick the setup that matches your environment.

## Option 1 — Docker (fastest; matches CI exactly)

```sh
# from the repo root
docker build -t tasks-dev -f desktop-native/docker/Dockerfile.dev .
docker run --rm -it -v "$PWD":/workspace -w /workspace/desktop-native tasks-dev bash
# inside the container:
cargo test --workspace
QT_QPA_PLATFORM=offscreen cargo run -p tasks-ui    # headless smoke
```

To see the live GUI from a Linux host, forward X:

```sh
docker run --rm -it \
  -e DISPLAY="$DISPLAY" \
  -v /tmp/.X11-unix:/tmp/.X11-unix \
  -v "$PWD":/workspace -w /workspace/desktop-native \
  tasks-dev \
  cargo run -p tasks-ui
```

The image is Ubuntu 24.04 + Qt 6.4.2 + Rust stable (clippy + rustfmt
included). See `docker/Dockerfile.dev` for the package list.

## Option 2 — bare Ubuntu 24.04 (including WSL2, Multipass, cloud VMs)

```sh
bash desktop-native/docker/setup-ubuntu-24.04.sh
```

Idempotent `apt-get` + `rustup` script. Identical package set to the
Dockerfile.

## Option 3 — other Linux distros

Fedora:

```sh
sudo dnf install -y qt6-qtbase-devel qt6-qtdeclarative-devel \
                    qt6-qttools-devel qt6-qtsvg-devel cmake clang \
                    pkgconf-pkg-config sqlite-devel
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Arch:

```sh
sudo pacman -S --needed qt6-base qt6-declarative qt6-tools qt6-svg \
                        cmake clang pkgconf sqlite rustup
rustup default stable
```

You also need the QtQuick QML modules for Controls, Layouts, Window,
and Dialogs — on Fedora/Arch those come with `qt6-qtdeclarative*`.

## Option 4 — macOS

```sh
brew install qt@6 cmake pkg-config sqlite rustup-init
rustup-init -y
export CMAKE_PREFIX_PATH="$(brew --prefix qt@6)"
export PATH="$(brew --prefix qt@6)/bin:$PATH"
```

Then the usual `cargo …` commands. The app runs natively via Qt on
macOS; `QT_QPA_PLATFORM=offscreen` works for headless smoke.

## Option 5 — Windows

1. Install Qt 6.4+ via the [Qt Online Installer] — pick the MSVC
   2022 64-bit component plus the QtQuick Controls / Dialogs modules.
2. Install Rust via [rustup] (`rustup-init.exe`, choose the MSVC
   toolchain).
3. Add `C:\Qt\6.x\msvc2022_64\bin` to your `PATH` (or set
   `CMAKE_PREFIX_PATH` to the same directory).
4. From a *Developer Command Prompt for VS 2022*, run the usual
   `cargo` commands from the `desktop-native` directory.

[Qt Online Installer]: https://www.qt.io/download-qt-installer
[rustup]: https://rustup.rs/

## Crate layout

```
desktop-native/
  Cargo.toml              # cargo workspace
  crates/
    tasks-core/           # pure Rust: models, DB open, queries, watcher
    tasks-ui/             # cxx-qt QObject + QML shell
      cxx/                # hand-written C++ shims (e.g. TaskListModelBase)
      qml/                # QML: Main.qml + three panes + PriorityDot
  docker/                 # reproducible toolchain recipes (see above)
  resources/              # icons, translations (empty at time of writing)
  packaging/{linux,macos,windows}/   # packaging stubs for future release work
```

## Runtime flags & environment variables

| Variable | Purpose |
|---|---|
| `QT_QPA_PLATFORM=offscreen` | Run the GUI without a display server. CI uses this; useful locally to verify the QML graph parses. |
| `QT_QUICK_CONTROLS_STYLE` | Override the QML control style. Default is `Material` (set by `src/main.rs` so `Material.theme: Material.System` tracks the OS color scheme); users can set this to `Fusion`, `Basic`, or `Universal`. |
| `RUST_LOG` | `tracing` filter. The binary initialises a `tracing-subscriber` in `src/main.rs`; `RUST_LOG=info` is the default, `debug` or `trace` for more detail. |
| `QT_LOGGING_RULES=qt.qml.*=true` | Makes Qt dump QML import resolution to stderr — useful for diagnosing missing `qml6-module-*` packages. |

## Dev loop details

### Fast inner loop

```sh
# pick ONE of these depending on what you touched
cargo test -p tasks-core         # no Qt needed; ~1 s rebuild
cargo test --workspace           # also recompiles cxx-qt; slower
cargo run  -p tasks-ui -- --cli fixtures.db   # smoke the data layer
```

### CI parity (runs the same thing `.github/workflows/desktop-native.yml` does)

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast
QT_QPA_PLATFORM=offscreen cargo run -p tasks-ui    # 3-second smoke
```

### Single-test debug

```sh
cargo test --workspace today_window_tests::utc_offset_returns_utc_midnight
cargo test --workspace --package tasks-core -- --nocapture   # print! visible
```

## Troubleshooting

**`could not find qmake6` / `Qt6Config.cmake: file not found`.** cxx-qt
uses `qmake` to discover Qt's include + lib paths. On Debian/Ubuntu,
install `qt6-base-dev`. On macOS, `export CMAKE_PREFIX_PATH="$(brew
--prefix qt@6)"`. On Windows, put `C:\Qt\6.x\msvc2022_64\bin` on
`PATH`.

**`module "QtQuick.Dialogs" is not installed`.** The QML runtime
package separate from Qt itself; install `qml6-module-qtquick-dialogs`
(Debian/Ubuntu) or the distro equivalent. Same pattern for Controls,
Layouts, Window, and the `Qt.labs.platform` module.

**`cargo test` is rebuilding cxx-qt on every invocation.** cxx-qt's
`build.rs` regenerates bindings when the bridge source changes. If
you're only iterating on `tasks-core`, use `cargo test -p tasks-core`
so `tasks-ui` stays off the build graph.

**`pending apt update` errors for `deadsnakes` / `ondrej/php`.** Not
ours; pre-existing PPAs that sometimes fail to sign. Disable via
`sudo mv /etc/apt/sources.list.d/deadsnakes-*.sources{,.disabled}`
before installing Qt.

**Schema drift (`SchemaMismatch` when opening a DB).**
`tasks_core::db::PINNED_IDENTITY_HASH` is pinned to a specific Room
version. If the Android client's schema changed upstream, a CI job
catches it (`tests/schema_guard.rs`); to unblock, bump both
`PINNED_SCHEMA_VERSION` and `PINNED_IDENTITY_HASH` and re-check that
entity column names still match `data/schemas/.../<ver>.json`.

**`QT_QPA_PLATFORM=offscreen` hangs forever.** The binary doesn't
auto-exit when idle. Use `timeout 3 cargo run -p tasks-ui` if you
just want a smoke check; exit code 124 (GNU) or 143 (SIGTERM) means
the process survived until the timeout expired, which is what you
want for "does QML parse?"

## See also

- `desktop-native/README.md` — high-level project intro and roadmap.
- `desktop-native/DECISIONS.md` — why specific technical choices were
  made.
- `desktop-native/PLAN_UPDATES.md` — amendments to the original plan
  at `/root/.claude/plans/i-m-interested-in-implementing-unified-parasol.md`.
- `.github/workflows/desktop-native.yml` — CI workflow: fmt, clippy,
  tests, and offscreen GUI smoke on Linux + macOS + Windows.
- `.github/workflows/desktop-native-release.yml` — release-build
  workflow. Triggers on tags matching `desktop-native-v*` (full
  release with draft GitHub Release) or on manual `workflow_dispatch`
  runs (one-off builds of a branch tip). Produces three artefacts
  per run:
    * Linux: `tasks-desktop-<version>-linux-x86_64.tar.gz`
    * macOS: `tasks-desktop-<version>-macos-universal.dmg` (bundled
      Qt libraries via `macdeployqt`)
    * Windows: `tasks-desktop-<version>-windows-x86_64.zip` (bundled
      DLLs + QML runtime via `windeployqt`)
  None of these are signed or notarized — on macOS clear the
  quarantine with `xattr -d com.apple.quarantine tasks-desktop.app`,
  on Windows expect a SmartScreen "unrecognized publisher" warning.
