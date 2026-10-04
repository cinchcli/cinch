{
  description = "Cinch: remote clipboard for developers";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs: {
        cinch = pkgs.callPackage ./nix/package.nix { };
        default = self.packages.${pkgs.stdenv.hostPlatform.system}.cinch;
      });

      overlays.default = final: _prev: {
        cinch = final.callPackage ./nix/package.nix { };
      };

      checks = forAllSystems (pkgs: {
        cinch = self.packages.${pkgs.stdenv.hostPlatform.system}.cinch;
      });
    };
}
