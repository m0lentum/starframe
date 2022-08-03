let
  sources = import ./nix/sources.nix;
  rust-overlay = import sources.rust-overlay;
  pkgs = import sources.nixpkgs { overlays = [ rust-overlay ]; };

  rust = pkgs.rust-bin.stable."1.62.1".default.override {
    targets = [ "wasm32-unknown-unknown" ];
  };
  # nightly if you need to e.g. test macros with unstable features:
  # rust = pkgs.rust-bin.selectLatestNightlyWith (toolchain: toolchain.default);
in
pkgs.mkShell {
  buildInputs = [
    pkgs.niv
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
    pkgs.xorg.libX11
  ];
  # make C libraries available
  LD_LIBRARY_PATH = with pkgs.xorg; with pkgs.lib.strings;
    concatStrings (intersperse ":" [
      "${libX11}/lib"
      "${libXcursor}/lib"
      "${libXxf86vm}/lib"
      "${libXi}/lib"
      "${libXrandr}/lib"
      "${pkgs.vulkan-loader}/lib"
      "${pkgs.stdenv.cc.cc.lib}/lib64"
    ]);
}
