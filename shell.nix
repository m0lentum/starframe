let
  sources = import ./nix/sources.nix;
  rust = import ./nix/rust.nix { inherit sources; };
  pkgs = import sources.nixpkgs {};
in
pkgs.mkShell {
  buildInputs = [
    rust
    # for wgpu
    pkgs.pkgconfig
    pkgs.xlibs.libX11
    pkgs.shaderc
  ];
  # make graphics libraries available
  LD_LIBRARY_PATH = with pkgs.xlibs; "${libX11}/lib:${libXcursor}/lib:${libXxf86vm}/lib:${libXi}/lib:${libXrandr}/lib:${pkgs.vulkan-loader}/lib";
}
