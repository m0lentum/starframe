{ sources ? import ./sources.nix }:

let
  pkgs =
    import sources.nixpkgs { overlays = [ (import sources.nixpkgs-mozilla) ]; };
in
pkgs.latest.rustChannels.stable.rust.override {
  extensions = [ "rust-src" ];
}