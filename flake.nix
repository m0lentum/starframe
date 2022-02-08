{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-21.11";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
    flake-utils.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        rust = pkgs.rust-bin.stable."1.58.0".default;
      in
      {
        devShell = pkgs.mkShell {
          buildInputs = [
            # rust and profiling
            rust
            pkgs.cargo-flamegraph
            pkgs.lld
            pkgs.llvmPackages.bintools
            pkgs.tracy
            # other utils
            pkgs.just
            pkgs.renderdoc
            # wgpu C dependencies
            pkgs.pkgconfig
            pkgs.xlibs.libX11
          ];
          # make C libraries available
          LD_LIBRARY_PATH = with pkgs.xlibs; with pkgs.lib.strings;
            concatStrings (intersperse ":" [
              "${libX11}/lib"
              "${libXcursor}/lib"
              "${libXxf86vm}/lib"
              "${libXi}/lib"
              "${libXrandr}/lib"
              "${pkgs.vulkan-loader}/lib"
              "${pkgs.stdenv.cc.cc.lib}/lib64"
            ]);
        };
      });
}
