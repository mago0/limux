# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Prerequisites before any build

`limux-ghostty-sys/build.rs` hard-fails if `ghostty/zig-out/lib/libghostty.so` is missing. A fresh clone will not compile until the Zig library is built:

```bash
git submodule update --init --recursive
(cd ghostty && zig build -Dapp-runtime=none -Doptimize=ReleaseFast)
```

The submodule path `ghostty/` points at a fork (`am-will/ghostty`), not upstream Ghostty. `build.rs` also compiles `ghostty/vendor/glad/src/gl.c` directly because `libghostty.so` does not bundle the GL loader when built as a shared library.

At runtime, the binary needs the shared library on its loader path:

```bash
LD_LIBRARY_PATH=../ghostty/zig-out/lib:$LD_LIBRARY_PATH ./target/release/limux
```

## Build contexts

Three contexts, don't mix them:

- **Dev loop** (tests, clippy, quick iteration): run from inside `nix-shell` for a pinned toolchain. Binaries produced inside nix-shell only run inside nix-shell - their ELF interpreter is `/nix/store/.../ld-linux-x86-64.so.2`, which deliberately ignores `/etc/ld.so.cache` and therefore can't find system GTK via the standard loader path.
- **Locally installable binary**: build *outside* nix-shell (`echo "${IN_NIX_SHELL:-no}"` must print `no`) against Debian `libgtk-4-dev libadwaita-1-dev libwebkitgtk-6.0-dev`. Run `cargo clean -p limux-host-linux` first to flush cached nix pkg-config outputs. Install with `sudo install -Dm755 target/release/limux /usr/bin/limux`; libghostty at `/usr/lib/limux/libghostty.so` is already on the loader path via `/etc/ld.so.conf.d/limux.conf`.
- **Distributable release**: `scripts/package.sh` (next section). Enforces a glibc baseline so artefacts work on older systems; skip for local installs.

## Canonical quality gate

`./scripts/check.sh` is the source-of-truth pre-commit check (mirrored by `.github/workflows/rust-quality.yml`). It runs:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Treat clippy findings as required maintainability work, not optional cleanup (per `docs/maintainability.md`).

### Running a single test

```bash
cargo test -p limux-core workspace_roundtrip         # one test in a crate
cargo test -p limux-control --test socket_roundtrip  # a specific integration-test file
```

## Release packaging

`./scripts/package.sh` rebuilds `libghostty.so` with `ReleaseFast -Dcpu=baseline`, assembles the tarball, and also drives `.deb`, `.rpm`, and `AppImage` outputs. It enforces `GLIBC <= 2.39` on the final binary via `objdump -T`; override intentionally with `LIMUX_MAX_GLIBC=<version>`. Release builds are pinned to Ubuntu 24.04 to keep that baseline; building on a newer distro will fail the check. `.cargo/config.toml` pins an rpath of `/usr/local/lib/limux` into the binary so installed builds find the bundled `libghostty.so`.

## Architecture

Cargo workspace with six crates; dependencies flow one way:

```
limux-protocol   wire types only (V2Request/V2Response + V1 envelope parser)
     |
limux-core       Dispatcher + entire state engine (workspaces/windows/panes/
     |           surfaces/browser/notifications). ~7k lines, intentionally
     |           a single source of truth per docs/maintainability.md.
     |
limux-control    Unix socket server, peer auth, socket-path resolution,
     |           framed request I/O. Exposes both rlib and staticlib.
     |           Also ships a standalone `limux-control-server` binary
     |           (for tests/headless; real clients hit the host bridge).
     |
limux-cli        `limux-cli` client binary (tokio UnixStream)
limux-host-linux `limux` GTK4/libadwaita UI binary (binary name: limux)
limux-ghostty-sys raw C FFI to libghostty.so
```

**Command model.** Every user-visible action routes through a V2 JSON-RPC-ish request (`{id, method, params}`) handled by `limux_core::Dispatcher`. The method table in `limux-core/src/lib.rs` (constant `COMMANDS`) is the canonical list; adding a new UI capability almost always means adding a method there and wiring it on both ends.

**Object hierarchy.** Workspace -> Window -> Pane -> Surface. Surfaces are the Ghostty-backed terminal or panel; panes hold tabs of surfaces; a window holds a split tree of panes; a workspace holds windows and carries a cwd.

**Control bridge (host).** `limux-host-linux/src/control_bridge.rs` brings the control socket into the GTK main loop: socket requests become `ControlCommand` messages sent to the GTK thread, which replies via `mpsc::Sender<BridgeResult>`. The host only implements a subset of methods (see its local `METHODS` array); the rest are handled by `limux-core`'s dispatcher logic. When adding a method that must touch live GTK widgets, extend `ControlCommand` + the bridge handler. When adding a pure-state method, extend the core dispatcher.

**Socket resolution.** `limux-control::socket_path` prefers an explicit `--socket`, then `$LIMUX_SOCKET`, then `$LIMUX_SOCKET_PATH`, then the mode default: `Runtime` uses `$XDG_RUNTIME_DIR/limux/limux.sock` (falling back to `/tmp/limux.sock`, locked down to `0700`/`0600`); `Debug` uses `/tmp/limux-debug.sock`. Peer auth in `auth.rs` reads `SO_PEERCRED` and honors `SocketControlMode` from env.

**Ghostty runtime environment.** `main.rs` in `limux-host-linux` does three things *before* GTK initializes, and reordering them breaks things:
1. Appends `gles-api,vulkan` to `GDK_DISABLE` / `GDK_DEBUG` - Ghostty requires desktop OpenGL; GDK will otherwise pick a GLES context.
2. Resolves `GHOSTTY_RESOURCES_DIR`, `TERMINFO`, `GHOSTTY_SHELL_INTEGRATION_XDG_DIR` by walking up from the exe (bundled `share/limux/ghostty` wins; dev checkouts fall through to `ghostty/zig-out/share/ghostty`; system `/usr/*` is the last resort). Existing env is replaced if invalid.
3. Sets `WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1` so WebKitGTK starts on systems without unprivileged user namespaces.

**Protocol versioning.** `parse_v1_command_envelope` accepts the legacy `{command|cmd|method, params|args|payload}` shape and promotes it to a V2 request. The server tries V2 parse first, then falls back. Keep new clients on V2.

## Where big things live

- Terminal surface embedding (Ghostty FFI, OpenGL area wiring): `limux-host-linux/src/terminal.rs` (~2k lines).
- Window layout, sidebar, split tree rendering, tabs: `window.rs` (~4.9k lines), `pane.rs` (~3.2k lines), `split_tree.rs`.
- Keybinding system: `shortcut_config.rs` defines the `ShortcutId` enum (one variant per bindable action, config at `$XDG_CONFIG_HOME/limux/shortcuts.json`); `keybind_editor.rs` is the in-app editor UI. There are planning docs at `shortcut-remap-plan.md`, `terminal-keybinds-settings-plan.md`, and `docs/shortcut-remap-testing.md`.
- Persistent layout (workspace/folder restore): `layout_state.rs`, `app_config.rs`.

## Repository conventions

From `docs/maintainability.md`:
- One source of truth for command metadata, flags, and business rules - extend the existing path, do not fork parallel ones.
- Prefer small domain modules over monolithic files; split by domain, not by vague helper names.
- Keep pure logic separate from GTK widget wiring where possible.
- Add regression tests when fixing behavior or moving high-risk logic.
- Do not commit generated artifacts, build outputs, or cache files.

The workspace version in `Cargo.toml` is the single version source; `scripts/package.sh` greps it out, and release workflows depend on that being authoritative. Bump it in one place.
