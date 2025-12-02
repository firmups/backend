{
  description = "FIRMUPS backend development environment";

  inputs.nixpkgs.url = "github:nixos/nixpkgs?ref=25.11";

  outputs = { self, nixpkgs }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs supportedSystems (system:
        f system
      );
    in {
      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
        in {
          default = pkgs.mkShell {
            name = "firmups-backend";
            buildInputs = with pkgs; [
              rustc
              cargo
              lldb_20
              diesel-cli
              libpq
              bashInteractive
            ];
            shellHook = ''
              export PS1="($name)$PS1"
              echo "Welcome to the $name devShell!"
            '';
          };
        }
      );
    };
}
