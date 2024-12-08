{
  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, flake-utils, nixpkgs }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        inherit (nixpkgs) lib;
        pkgs = nixpkgs.legacyPackages.${system};
        rpath = lib.makeLibraryPath (with pkgs; [

        ]);
      in
      {
         packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "vtracer";
          name= "vtracer";
          src = self; 

          inherit ((lib.importTOML (self + "/Cargo.toml"))) ;

          cargoLock.lockFile = self + "/Cargo.lock";

          # cargoHash= "sha256-iC62yEpscSg3tzmbi78bNAcFRKI7w26N+bmFlAeiG1s=";
          nativeBuildInputs = with pkgs; [
            pkg-config
          ];

          meta = {
          mainProgram = "vtracer";
          description = "Raster to Vector Graphics Converter ";
          homepage = "https://github.com/visioncortex";
          license = pkgs.lib.licenses.mit;
        };
        };

        devShells.default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            rustc
            cargo
            pkg-config
          ];

          LD_LIBRARY_PATH = rpath;
        };
      }
    );
}