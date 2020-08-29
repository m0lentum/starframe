let
  moz-overlay = import (fetchTarball {
    url = "https://github.com/mozilla/nixpkgs-mozilla/archive/e912ed483e980dfb4666ae0ed17845c4220e5e7c.tar.gz";
    sha256 = "08fvzb8w80bkkabc1iyhzd15f4sm7ra10jn32kfch5klgl0gj3j3";
  });

  pkgs = import (fetchTarball {
    # nixos-20.03
    url = "https://github.com/NixOS/nixpkgs-channels/archive/ab3adfe1c769c22b6629e59ea0ef88ec8ee4563f.tar.gz";
    sha256 = "1m4wvrrcvif198ssqbdw897c8h84l0cy7q75lyfzdsz9khm1y2n1";
  }) { overlays = [ moz-overlay ]; };

  rust = (pkgs.rustChannelOf { channel = "1.45.0"; }).rust.override {
    extensions = [ "rust-src" ];
  };
in
pkgs.mkShell {
  buildInputs = with pkgs; [
    rust
    # for wgpu
    pkgconfig
    xlibs.libX11
    shaderc
  ];
  # make graphics libraries available
  LD_LIBRARY_PATH = with pkgs.xlibs; "${libX11}/lib:${libXcursor}/lib:${libXxf86vm}/lib:${libXi}/lib:${libXrandr}/lib:${pkgs.vulkan-loader}/lib";
}
