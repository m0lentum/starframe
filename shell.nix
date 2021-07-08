let
  sources = import ./nix/sources.nix;

  pkgs = import sources.nixpkgs { overlays = [ (import sources.nixpkgs-mozilla) ]; };

  rust = (pkgs.rustChannelOf { channel = "1.52.1"; }).rust.override {
    extensions = [ "rust-src" ];
  };
in
pkgs.mkShell {
  buildInputs = [
    pkgs.niv
    rust
    pkgs.cargo-flamegraph
    # for wgpu
    pkgs.pkgconfig
    pkgs.xlibs.libX11
    pkgs.shaderc
  ];
  # make graphics libraries available
  LD_LIBRARY_PATH = with pkgs.xlibs; "${libX11}/lib:${libXcursor}/lib:${libXxf86vm}/lib:${libXi}/lib:${libXrandr}/lib:${pkgs.vulkan-loader}/lib";
}
