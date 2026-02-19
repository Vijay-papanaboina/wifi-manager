{
  description = "A lightweight WiFi manager for Wayland compositors";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
    utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, crane, utils, ... }:
    utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };

        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        # System dependencies
        buildInputs = with pkgs; [
          gtk4
          gtk4-layer-shell
          networkmanager # for libnm
          glib
          cairo
          pango
          gdk-pixbuf
        ];

        nativeBuildInputs = with pkgs; [
          pkg-config
          wrapGAppsHook4
        ];

        commonArgs = {
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              (pkgs.lib.hasInfix "/resources/" path) ||
              (craneLib.filterCargoSources path type);
          };
          strictDeps = true;

          inherit buildInputs nativeBuildInputs;
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        wifi-manager = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
        });
      in
      {
        packages.default = wifi-manager;

        devShells.default = craneLib.devShell {
          inputsFrom = [ wifi-manager ];
          packages = with pkgs; [
            # Add any additional dev tools here
          ];
        };
      }
    );
}
