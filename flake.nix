{
  description = "Crate — Tauri v2 music library manager: dev shells (native + Windows cross-build)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};

      # Unprefixed aliases (as/ld/ar/windres/dlltool/gcc...) that resolve to the
      # mingw-w64 toolchain. `nix develop` is NOT path-pure: host binutils' bare `as`
      # otherwise wins gcc's PATH lookup and the vendored OpenSSL build dies on
      # `as: unrecognized option '-mbig-obj'` (PE flag). Dev-shell PATH prepends
      # shell packages, so these aliases shadow the host tools inside the shell.
      winCrossNativeNames = pkgs.runCommand "crate-win-cross-native-names" { }
        ''
          mkdir -p $out/bin
          b=${pkgs.pkgsCross.mingwW64.buildPackages.binutils}
          g=${pkgs.pkgsCross.mingwW64.buildPackages.gcc}
          for t in as ld ar nm ranlib objcopy objdump strip dlltool windres windmc size; do
            [ -x "$b/bin/x86_64-w64-mingw32-$t" ] && ln -s "$b/bin/x86_64-w64-mingw32-$t" "$out/bin/$t"
          done
          for t in gcc g++; do
            ln -s "$g/bin/x86_64-w64-mingw32-$t" "$out/bin/$t"
          done
        '';

      # Rust std for windows-gnu links `-l:libpthread.a` (winpthreads). nixpkgs' mingw
      # gcc does not carry it in its search path (verified: -print-file-name returns
      # the bare name), so expose the pthreads package lib dir via RUSTFLAGS -L.
      # NOTE: the pthreads drv must NOT be added to `packages`/nativeBuildInputs of
      # this native shell — its meta.platforms=windows makes mkShell's platform check
      # refuse it. A string reference only is fine and is what works.
      winPthreads = pkgs.pkgsCross.mingwW64.windows.pthreads;
    in
    {
      devShells.${system} = {

        # Native development: `yarn dev`, `yarn check`, cargo/clippy/rustfmt.
        # Node 26 + Yarn 4 come from mise (repo mise.toml); Rust toolchains from the
        # global rustup (pinned by src-tauri/rust-toolchain.toml to nightly-2026-02-19).
        # This shell layers on top: the Tauri/wry native stack (webkitgtk API 4.1 +
        # libsoup 3) and build utilities.
        default = pkgs.mkShell {
          name = "crate-dev";
          packages = with pkgs; [
            pkg-config
            glib
            gtk3
            webkitgtk_4_1
            libsoup_3
            glib-networking # TLS backend used by libsoup (update checks, OAuth flows)
            rustup # proxy shims that resolve the repo-pinned toolchain
            git
          ];
          shellHook = ''
            echo "[crate-dev] node/yarn via mise; cargo/rustc via rustup (pinned nightly-2026-02-19)"
            echo "[crate-dev] try: yarn install --frozen-lockfile && yarn dev"
          '';
        };

        # Cross-build for Windows without a VM:
        #   nix develop .#win-cross
        #   cargo build --release --features desktop --target x86_64-pc-windows-gnu \
        #     --manifest-path src-tauri/Cargo.toml   (or: scripts/win-cross.sh)
        #
        # Tools:
        #   * mingw-w64 gcc + binutils: x86_64-w64-mingw32-gcc / windres / ar — what
        #     rust's windows-gnu target and tauri-build's embed-manifest expect.
        #   * nasm: assembles ring + vendored OpenSSL on x86 windows.
        #   * perl: Configure step of the vendored OpenSSL build inside
        #     rusqlite/bundled-sqlcipher-vendored-openssl (why CI installs Strawberry Perl).
        #
        # Output is the bare crate-app.exe. MSI/NSIS installers cannot be bundled from
        # Linux (the Tauri bundler requires a Windows host for WiX/NSIS) — for those use
        # the `test.release.yml` / `cd.release.yml` workflows (windows-latest).
        win-cross = pkgs.mkShell {
          name = "crate-win-cross";
          # NOTE: winPthreads is deliberately NOT in packages — mkShell meta.platforms
          # checks would reject a windows-platform package in a linux shell. It is only
          # referenced (via RUSTFLAGS -L) so the linker finds libpthread.a.
          packages = with pkgs; [
            winCrossNativeNames
            pkgsCross.mingwW64.buildPackages.gcc
            pkgsCross.mingwW64.buildPackages.binutils
            nasm
            perl
            pkg-config
            rustup
            git
            gnumake
          ];
          CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = "x86_64-w64-mingw32-gcc";
          RUSTFLAGS = "-L ${winPthreads}/lib";
          shellHook = ''
            # Ensure the windows-gnu std exists for BOTH the default toolchain and the
            # repo-pinned one (src-tauri/rust-toolchain.toml overrides only under src-tauri).
            rustup target add x86_64-pc-windows-gnu >/dev/null 2>&1 \
              || echo "[win-cross] WARN: target add (default toolchain) failed (offline?)"
            rustup target add x86_64-pc-windows-gnu --toolchain nightly-2026-02-19 >/dev/null 2>&1 \
              || echo "[win-cross] NOTE: repo-pinned nightly not installed locally; it will fetch it on first cargo run (needs network once)"
            echo "[win-cross] mingw gcc: $(x86_64-w64-mingw32-gcc -dumpversion 2>/dev/null || echo missing)"
            echo "[win-cross] run: scripts/win-cross.sh   (or cargo build --release --features desktop --target x86_64-pc-windows-gnu --manifest-path src-tauri/Cargo.toml)"
          '';
        };
      };

      formatter.${system} = pkgs.nixfmt-rfc-style;

      # Run the full Windows cross-build from the host: nix run .#win-cross
      apps.${system} = {
        win-cross = {
          type = "app";
          program = "${toString ./.}/scripts/win-cross.sh";
        };
      };
    };
}
