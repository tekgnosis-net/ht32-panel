{
  description = "HT32 Panel - Mini PC Display & LED Control";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    git-hooks = {
      url = "github:cachix/git-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, git-hooks, rust-overlay }:
    let
      cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
      version = cargoToml.workspace.package.version;
    in
    flake-utils.lib.eachSystem [ "x86_64-linux" ] (system:
      let
        overlays = [ rust-overlay.overlays.default ];
        pkgs = import nixpkgs { inherit system overlays; };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" "clippy" "rustfmt" ];
        };

        nativeBuildInputs = with pkgs; [
          pkg-config
          cmake
        ];

        buildInputs = with pkgs; [
          hidapi
          libusb1
          udev
          systemd
          dbus
        ];

        cargoArgs = {
          pname = "ht32-panel";
          inherit version;
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          inherit nativeBuildInputs buildInputs;
          cargoTestFlags = [ "--workspace" "--" "--skip" "test_device_open" ];

          meta = with pkgs.lib; {
            description = "HT32 Panel - Mini PC Display & LED Control";
            homepage = "https://github.com/tekgnosis-net/ht32-panel";
            license = licenses.agpl3Plus;
            platforms = [ "x86_64-linux" ];
          };
        };

        pre-commit = git-hooks.lib.${system}.run {
          src = ./.;
          hooks = {
            check-json.enable = true;
            check-merge-conflicts.enable = true;
            check-toml.enable = true;
            check-yaml.enable = true;
            clippy = {
              enable = true;
              entry = let
                depsPath = pkgs.lib.makeBinPath ([ rustToolchain ] ++ nativeBuildInputs);
                pkgConfigPath = pkgs.lib.makeSearchPath "lib/pkgconfig"
                  (map (p: if p ? dev then p.dev else p) buildInputs);
              in toString (pkgs.writeShellScript "clippy-hook" ''
                export PATH="${depsPath}''${PATH:+:$PATH}"
                export PKG_CONFIG_PATH="${pkgConfigPath}"
                cargo clippy --workspace --all-targets --offline -- -D warnings
              '');
              files = "\\.rs$";
              types = [ "file" ];
              pass_filenames = false;
            };
            detect-private-keys.enable = true;
            end-of-file-fixer.enable = true;
            rustfmt = {
              enable = true;
              packageOverrides.cargo = rustToolchain;
              packageOverrides.rustfmt = rustToolchain;
            };
            trim-trailing-whitespace.enable = true;
          };
        };

      in {
        packages = {
          default = pkgs.rustPlatform.buildRustPackage (cargoArgs // {
            postInstall = ''
              mkdir -p $out/share/ht32-panel
              cp -r config $out/share/ht32-panel/
            '';
          });

          ht32paneld = pkgs.rustPlatform.buildRustPackage (cargoArgs // {
            pname = "ht32paneld";
            cargoBuildFlags = [ "-p" "ht32-panel-daemon" ];
            postInstall = ''
              mkdir -p $out/share/ht32-panel
              cp -r config $out/share/ht32-panel/
            '';
          });

          ht32panelctl = pkgs.rustPlatform.buildRustPackage (cargoArgs // {
            pname = "ht32panelctl";
            cargoBuildFlags = [ "-p" "ht32-panel-cli" ];
          });

          release-tarball = let
            pkg = self.packages.${system}.default;
          in pkgs.runCommand "ht32-panel-${version}-x86_64-linux.tar.gz" {
            nativeBuildInputs = [ pkgs.gzip pkgs.patchelf ];
          } ''
            mkdir -p dist/config
            cp ${pkg}/bin/ht32paneld dist/
            cp ${pkg}/bin/ht32panelctl dist/
            chmod +w dist/ht32paneld dist/ht32panelctl
            patchelf --remove-rpath dist/ht32paneld
            patchelf --remove-rpath dist/ht32panelctl
            cp -r ${pkg}/share/ht32-panel/config/* dist/config/
            tar -czvf $out -C dist .
          '';

          # Combined release with all artifacts for Garnix
          release = let
            tarball = self.packages.${system}.release-tarball;
          in pkgs.runCommand "ht32-panel-${version}-release" {} ''
            mkdir -p $out
            cp ${tarball} $out/ht32-panel-${version}-x86_64-linux.tar.gz
          '';
        };

        checks = {
          fmt = pkgs.runCommand "check-fmt" {
            nativeBuildInputs = [ rustToolchain ];
            src = self;
          } ''
            cd $src
            cargo fmt --all -- --check
            touch $out
          '';

          clippy = pkgs.rustPlatform.buildRustPackage (cargoArgs // {
            pname = "ht32-panel-clippy";
            nativeBuildInputs = nativeBuildInputs ++ [
              pkgs.clippy
              pkgs.rustPlatform.cargoSetupHook
            ];
            buildInputs = buildInputs;
            buildPhase = ''
              runHook preBuild
              cargo clippy --workspace --all-targets --offline -- -D warnings
              runHook postBuild
            '';
            installPhase = "mkdir -p $out";
            doCheck = false;
          });

          tests = self.packages.${system}.default;
        };

        devShells.default = pkgs.mkShell {
          name = "ht32-panel-dev";

          packages = [
            rustToolchain
          ] ++ (with pkgs; [
            # Development tools
            cargo-nextest
            cargo-watch
            cargo-audit
            cargo-outdated
            just
            watchexec

            # Python (for flatpak-cargo-generator)
            (python3.withPackages (ps: [ ps.aiohttp ps.toml ]))
          ]) ++ nativeBuildInputs ++ buildInputs;

          RUST_BACKTRACE = "1";
          RUST_LOG = "info";

          shellHook = ''
            ${pre-commit.shellHook}
            echo ""
            echo "HT32 Panel Development Environment"
            echo ""
            echo "Build:    cargo build --workspace"
            echo "Test:     cargo nextest run --workspace"
            echo "Lint:     cargo clippy --workspace --all-targets -- -D warnings"
            echo "Format:   cargo fmt --all"
            echo "Daemon:   cargo run -p ht32-panel-daemon -- config/default.toml"
            echo ""
          '';
        };
      }
    ) // {
      # NixOS modules (system-level service)
      nixosModules.default = { config, lib, pkgs, ... }: {
        imports = [ ./nix/module.nix ];
        config = lib.mkIf config.services.ht32-panel.enable {
          services.ht32-panel.package = lib.mkDefault self.packages.${pkgs.stdenv.hostPlatform.system}.default;
        };
      };
      nixosModules.ht32-panel = self.nixosModules.default;

      # Standalone udev rules module (for use with Home Manager)
      # Import this in your NixOS config when using the Home Manager module
      nixosModules.udevRules = { config, lib, ... }:
        let
          cfg = config.services.ht32-panel.udevRules;
        in {
          options.services.ht32-panel.udevRules = {
            enable = lib.mkEnableOption "udev rules for HT32 Panel hardware access";

            group = lib.mkOption {
              type = lib.types.str;
              default = "users";
              description = "Group to grant access to hardware devices.";
            };
          };

          config = lib.mkIf cfg.enable {
            services.udev.extraRules = ''
              # HT32 Panel LCD (VID:PID 04D9:FD01)
              SUBSYSTEM=="usb", ATTR{idVendor}=="04d9", ATTR{idProduct}=="fd01", MODE="0660", GROUP="${cfg.group}"
              SUBSYSTEM=="hidraw", ATTRS{idVendor}=="04d9", ATTRS{idProduct}=="fd01", MODE="0660", GROUP="${cfg.group}"

              # CH340 serial adapter for LED strip
              SUBSYSTEM=="tty", ATTRS{idVendor}=="1a86", ATTRS{idProduct}=="7523", MODE="0660", GROUP="${cfg.group}", SYMLINK+="ht32-led"
            '';
          };
        };

      # Home Manager modules (user-level service)
      homeManagerModules.default = { config, lib, pkgs, osConfig ? null, ... }: {
        imports = [ ./nix/home-module.nix ];
        config = lib.mkIf config.services.ht32-panel.enable {
          services.ht32-panel.package = lib.mkDefault self.packages.${pkgs.stdenv.hostPlatform.system}.default;
          services.ht32-panel.cli.package = lib.mkDefault self.packages.${pkgs.stdenv.hostPlatform.system}.ht32panelctl;
        };
      };
      homeManagerModules.ht32-panel = self.homeManagerModules.default;

      overlays.default = final: prev: {
        ht32-panel = self.packages.${prev.system}.default;
        ht32paneld = self.packages.${prev.system}.ht32paneld;
        ht32panelctl = self.packages.${prev.system}.ht32panelctl;
      };
    };
}
