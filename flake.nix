{
  description = "FIRMUPS backend development environment";

  inputs.nixpkgs.url = "github:nixos/nixpkgs?ref=25.11";

  outputs =
    { self, nixpkgs }:
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
      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          default = pkgs.mkShell {
            name = "firmups-backend";
            buildInputs = with pkgs; [
              rustc
              cargo
              rustfmt
              lldb_20
              diesel-cli
              libpq
              bashInteractive
              nixfmt-rfc-style
            ];
            shellHook = ''
              export PS1="($name)$PS1"
              echo "Welcome to the $name devShell!"
            '';
          };
        }
      );

      packages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          rustPlatform = pkgs.rustPlatform;
        in
        {
          # Build your Rust crate/workspace with `nix build .#backend`
          backend = rustPlatform.buildRustPackage {
            pname = "firmups-backend";
            version = "0.1.0";

            # Build from the repo root (flake directory)
            src = ./.;

            # Use cargoHash for modern nixpkgs (>= 23.11). It vendors crates automatically.
            # First run with a dummy hash (sha256-AAAAAAAA...) to get the correct hash from the error.
            cargoHash = "sha256-DM4itoS4SyadoigqXioBxW9HX35JwLwhrow4BkrcUmY=";

            # If your project needs system libs (e.g., libpq for Diesel)
            buildInputs = with pkgs; [
            ];
            nativeBuildInputs = with pkgs; [
              pkg-config
            ];

            # If your crate name is NOT `firmups-backend`, set cargoBuildFlags or override Cargo.toml accordingly.
            # cargoBuildFlags = [ "--package" "your-crate-name" ];

            # If you need to enable/disable features:
            # buildFeatures = [ "some-feature" ];
            # buildNoDefaultFeatures = true;
            # buildFeatures = [ "default" "extra" ];

            # Optionally expose a binary name if your Cargo produces it
            # (buildRustPackage detects it automatically if Cargo.toml has [[bin]]).
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
            name = "firmups-backend";
            tag = "v0.1.0";

            contents = [ 
              backend
              pkgs.busybox
              pkgs.diesel-cli
            ];

            config = {
              Cmd = [ "${pkgs.busybox}/bin/sh" "-c" "${pkgs.diesel-cli}/bin/diesel migration run && ${backend}/bin/firmups-backend" ];
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
