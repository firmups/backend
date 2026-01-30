{
  description = "FIRMUPS backend development environment";

  inputs.nixpkgs.url = "github:nixos/nixpkgs?ref=25.11";
  inputs.git-hooks.url = "github:cachix/git-hooks.nix";
  inputs.rust-overlay = {
    url = "github:oxalica/rust-overlay";
    inputs.nixpkgs.follows = "nixpkgs";
  };
  outputs =
    {
      self,
      nixpkgs,
      git-hooks,
      rust-overlay,
    }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs supportedSystems (system: f system);
    in
    {
      checks = forAllSystems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };
          rustToolchain = pkgs.rust-bin.stable.latest.default.override {
            extensions = [
              "clippy"
              "rustfmt"
              "rust-src"
            ];
          };
        in
        {
          pre-commit-check = git-hooks.lib.${system}.run {
            src = ./.;
            hooks = {
              nixfmt-rfc-style.enable = true;
              cargo-clippy = {
                enable = true;
                name = "cargo clippy";
                entry = "${rustToolchain}/bin/cargo clippy --all-features --all-targets -- -D warnings";
                language = "system";
                pass_filenames = false;
              };
              cargo-fmt = {
                enable = true;
                name = "cargo fmt (check)";
                entry = "${rustToolchain}/bin/cargo fmt --all -- --check";
                language = "system";
                pass_filenames = false;
              };
              cargo-check = {
                enable = true;
                name = "cargo check";
                entry = "${rustToolchain}/bin/cargo check --all-targets --all-features";
                language = "system";
                pass_filenames = false;
              };
            };
          };
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };
          rustToolchain = pkgs.rust-bin.stable.latest.default.override {
            extensions = [
              "clippy"
              "rustfmt"
              "rust-src"
            ];
          };
          preCommit = self.checks.${system}.pre-commit-check;
        in
        {
          default = pkgs.mkShell {
            name = "firmups-backend";
            buildInputs = with pkgs; [
              rustToolchain
              rust-analyzer
              lldb_20
              diesel-cli
              libpq
              bashInteractive
              nixfmt-rfc-style
            ];
            packages = [ preCommit.enabledPackages ];
            shellHook = ''
              # Enable git hooks
              ${preCommit.shellHook}
              export PS1="($name)$PS1"
              echo "Welcome to the $name devShell!"
            '';
          };
        }
      );

      packages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };
          rustToolchain = pkgs.rust-bin.stable.latest.default.override {
            extensions = [
              "clippy"
              "rustfmt"
              "rust-src"
            ];
          };
          rustPlatform = pkgs.makeRustPlatform {
            rustc = rustToolchain;
            cargo = rustToolchain;
          };
        in
        {
          # Build your Rust crate/workspace with `nix build .#backend`
          backend = rustPlatform.buildRustPackage {
            pname = "firmups-backend";
            version = "0.1.1";

            # Build from the repo root (flake directory)
            src = ./.;

            # Use cargoHash for modern nixpkgs (>= 23.11). It vendors crates automatically.
            # First run with a dummy hash (sha256-AAAAAAAA...) to get the correct hash from the error.
            cargoHash = "sha256-fV+0nP34RPjk/WgEBmWw5kwZDGLM3p9C8a55e0AkfL8=";

            buildInputs = with pkgs; [
            ];
            nativeBuildInputs = with pkgs; [
              pkg-config
            ];

            # If your crate name differs from `firmups-backend`, adjust flags:
            # cargoBuildFlags = [ "--package" "your-crate-name" ];

            # Feature toggles if needed:
            # buildNoDefaultFeatures = true;
            # buildFeatures = [ "default" "extra" ];
          };
          dockerImage = self.dockerImages.${system}.dockerImage;

          # Make it the default build target so `nix build` (without attr) works.
          default = pkgs.symlinkJoin {
            name = "default";
            paths = [ (pkgs.lib.getOutput "out" self.packages.${system}.backend) ];
          };
        }
      );

      dockerImages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          backend = self.packages.${system}.backend;
        in
        {
          dockerImage = pkgs.dockerTools.buildLayeredImage {
            name = "firmups-backend-docker";
            tag = "v0.1.1";

            contents = [
              backend
              pkgs.busybox
              pkgs.diesel-cli
            ];

            config = {
              Cmd = [
                "${pkgs.busybox}/bin/sh"
                "-c"
                "${pkgs.diesel-cli}/bin/diesel migration run && ${backend}/bin/firmups-backend"
              ];
              User = "65532:65532"; # nobody
              WorkingDir = "/opt/firmups";
            };

            fakeRootCommands = ''
              mkdir -p ./opt/firmups
              mkdir -p ./opt/firmups/data
              cp -r ${./migrations} ./opt/firmups/migrations
              chown -R 65532:65532 ./opt/firmups
            '';
          };
        }
      );

      formatter = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        pkgs.nixfmt-rfc-style
      );
    };
}
