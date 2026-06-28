{
  description = "A riscv kernel, written mostly in Rust";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, crane }:
      flake-utils.lib.eachSystem
      (with flake-utils.lib.system; [ x86_64-linux ])
      (system: 
        let
          overlays = [ (import rust-overlay) ];
          pkgs = import nixpkgs { inherit system overlays; };
          riscvPkgs = pkgs.pkgsCross.riscv64;
          # bpf-linker has a hard requirement on llvm 21
          llvmPackages = pkgs.llvmPackages_21;
          # this specific rust version is built on llvm 21, DO NOT blindly upgrade
          # or ebpf compilation will break
          rust-toolchain = pkgs.rust-bin.nightly."2025-12-15".default.override {
            extensions = [
              "rust-src"
              "rust-analyzer"
            ];
            targets = [
              "riscv64gc-unknown-none-elf"
            ];
          };

          rustfmt = pkgs.rust-bin.stable.latest.rustfmt;

          rustPlatform = pkgs.makeRustPlatform {
            cargo = rust-toolchain;
            rustc = rust-toolchain;
          };

          craneLib = (crane.mkLib pkgs).overrideToolchain rust-toolchain;

          bindgen = rustPlatform.buildRustPackage rec {
            pname = "bindgen-cli";
            version = "v0.72.1";
            src = pkgs.fetchFromGitHub {
              owner = "rust-lang";
              repo = "rust-bindgen";
              rev = "27577d2930af9311495b0d0f016f903824521ddc";
              sha256 = "sha256-cswBbshxTAcZtUk3PxH9jD55X1a/fBAyZyYpzVkt27M=";
            };

            cargoLock = {
              lockFile = "${src}/Cargo.lock";
            };

            nativeBuildInputs = [ pkgs.pkg-config ];

            buildInputs = [ pkgs.openssl ];

            doCheck = false;
          };

        riscv-toolchain =
          import nixpkgs {
            localSystem = "${system}";
            crossSystem = {
              config = "riscv64-none-elf";
              libc = "newlib-nano";
              abi = "ilp32";
            };
          };

        riscv-toolchain-linux =
          import nixpkgs {
            localSystem = "${system}";
            crossSystem = {
              config = "riscv64-linux-gnu";
            };
          };
        in
    {
    devShells.default = pkgs.mkShell {
      packages = with pkgs; [
        clang
        openssl
        pkg-config
        qemu
        zig
        zls
      ] ++ [
        rust-toolchain
        bindgen
        rustfmt
        riscv-toolchain.buildPackages.binutils
        riscv-toolchain-linux.buildPackages.gcc
      ];

      shellHook = ''
        export LIBCLANG_PATH=${llvmPackages.libclang.lib}/lib
      '';
    };

  packages.riscv-opensbi = riscvPkgs.opensbi;
  packages.riscv-kernel = import ./kernel/package.nix {
    inherit pkgs craneLib;
  };
  packages.default = self.packages.${system}.riscv-kernel;
  });
}
