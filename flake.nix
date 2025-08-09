{
  description = "G-code Toolkit";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs";

  outputs =
    { self, nixpkgs }:
    {
      packages.x86_64-linux.default =
        let
          pkgs = import nixpkgs {
            system = "x86_64-linux";
          };
        in
        pkgs.rustPlatform.buildRustPackage {
          pname = "gctk";
          version = "0.1.0";
          src = ./.;
          cargoLock = {
            lockFile = ./Cargo.lock;
            outputHashes = {
              "gcode-0.6.2-alpha.0" = "sha256-th75m5LRrX7K6kDyvK80e48zibZjZabRSJ3hlSQ1kzU=";
            };
          };
        };
    };
}
