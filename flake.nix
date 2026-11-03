{
  description = "SmartThermWeb thermostat dashboard";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      eachSystem = nixpkgs.lib.genAttrs systems;
    in {
      packages = eachSystem (system: {
        default = nixpkgs.legacyPackages.${system}.callPackage ./nix/package.nix { };
      });
      nixosModules.default = import ./nix/module.nix;
      checks = eachSystem (system: {
        package = self.packages.${system}.default;
      });
    };
}
