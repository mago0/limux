# Isolated dev environment for limux.
#
# Usage: `nix-shell` in the repo root, or `nix-shell --run './scripts/check.sh'`.
#
# Why nix-shell over devbox: devbox only surfaces the `.out` outputs of multi-
# output Nix packages, so the `-dev` outputs that ship pkg-config files (gtk4,
# libadwaita, webkitgtk, fontconfig, etc.) aren't visible. mkShell + buildInputs
# handles that automatically via the standard pkg-config setup hook.
{ pkgs ? import (builtins.fetchTarball {
    # Same nixpkgs revision devbox generated for this project.
    url = "https://github.com/NixOS/nixpkgs/archive/566acc07c54dc807f91625bb286cb9b321b5f42a.tar.gz";
  }) {}
}:

pkgs.mkShell {
  name = "limux-dev";

  nativeBuildInputs = [
    pkgs.pkg-config
    pkgs.blueprint-compiler
  ];

  buildInputs = [
    # Rust toolchain comes from rustup (~/.cargo/bin), NOT nixpkgs. The nixpkgs
    # rustc had a propagation issue where `cargo:rustc-link-lib=static=glad`
    # from limux-ghostty-sys's build.rs never reached the downstream binary's
    # rustc command, breaking libghostty linking. Rustup-installed stable
    # behaves identically to CI and propagates the directive correctly.

    # Zig for building ghostty.
    pkgs.zig_0_15

    # Linker / C toolchain.
    pkgs.gcc
    pkgs.git

    # GTK stack needed to link limux-host-linux.
    pkgs.gtk4
    pkgs.libadwaita
    pkgs.webkitgtk_6_0
    pkgs.libepoxy
    pkgs.glib
    pkgs.pango
    pkgs.cairo
    pkgs.gdk-pixbuf
    pkgs.graphene

    # Ghostty zig-build pulls these from the system.
    pkgs.fontconfig
    pkgs.freetype
    pkgs.harfbuzz

    pkgs.openssl
  ];

  # Expose the runtime .so paths from buildInputs so `cargo test` binaries
  # can dlopen libgtk-4.so.1 etc. mkShell exports buildInputs to
  # $NIX_LD_LIBRARY_PATH via the stdenv setup hook; propagate it.
  shellHook = ''
    # Put rustup's toolchain first so we use stable from ~/.cargo/bin.
    if [ -f "$HOME/.cargo/env" ]; then
      . "$HOME/.cargo/env"
    fi
    RUNTIME_LIBS=""
    for p in ${builtins.concatStringsSep " " (map (p: "${p.out or p}/lib") (with pkgs; [
      gtk4 libadwaita webkitgtk_6_0 libepoxy glib pango cairo gdk-pixbuf graphene
      fontconfig freetype harfbuzz openssl
    ]))}; do
      RUNTIME_LIBS="$p:$RUNTIME_LIBS"
    done
    export LD_LIBRARY_PATH="$PWD/ghostty/zig-out/lib:$RUNTIME_LIBS''${LD_LIBRARY_PATH:-}"
    export LIMUX_SHELL_NIX=1
    echo "limux dev shell: rustc $(rustc --version | awk '{print $2}') / zig $(zig version)"
  '';
}
