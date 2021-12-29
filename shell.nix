let
  sources = import ./nix/sources.nix;

  pkgs = import sources.nixpkgs { overlays = [ (import sources.nixpkgs-mozilla) ]; };

  rust = (pkgs.rustChannelOf { channel = "1.56.1"; }).rust.override {
    extensions = [ "rust-src" ];
  };
in
pkgs.mkShell {
  buildInputs = [
    rust
    pkgs.cargo-flamegraph
    pkgs.cargo-outdated
    pkgs.lld
    pkgs.llvmPackages.bintools

    pkgs.niv
    pkgs.tracy
    pkgs.just

    # wgpu dependencies
    pkgs.pkgconfig
    pkgs.xlibs.libX11
    pkgs.shaderc
  ];
  # make libraries available
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
}
