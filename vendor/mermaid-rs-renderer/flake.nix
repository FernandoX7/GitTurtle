{
  description = "Fast Mermaid diagram renderer in pure Rust - 23 diagram types, 100-1400x faster than mermaid-cli";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
        mmdr = pkgs.rustPlatform.buildRustPackage {
          pname = "mermaid-rs-renderer";
          version = cargoToml.package.version;
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [ pkgs.pkg-config ];
          # mmdr is pure Rust; font discovery happens at runtime via fontdb.
          # fontconfig/freetype are listed for Linux so system fonts resolve
          # in PNG rendering environments that expect them.
          buildInputs =
            pkgs.lib.optionals pkgs.stdenv.isLinux [
              pkgs.fontconfig
              pkgs.freetype
            ]
            ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
              pkgs.libiconv
            ];
          doCheck = false;
          meta = {
            description = "Fast Mermaid diagram renderer in pure Rust - 23 diagram types, 100-1400x faster than mermaid-cli";
            homepage = "https://github.com/1jehuang/mermaid-rs-renderer";
            license = pkgs.lib.licenses.mit;
            mainProgram = "mmdr";
          };
        };
      in
      {
        packages.default = mmdr;
        apps.default = {
          type = "app";
          program = "${mmdr}/bin/mmdr";
          meta = mmdr.meta;
        };
        devShells.default = pkgs.mkShell {
          inputsFrom = [ mmdr ];
          packages = [ pkgs.rustc pkgs.cargo pkgs.clippy pkgs.rustfmt ];
        };
      }
    );
}
