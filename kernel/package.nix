{ pkgs, craneLib }:

let
  src = pkgs.lib.cleanSourceWith {
    src = craneLib.path ./..;
    filter =
      path: type:
      (craneLib.filterCargoSources path type)
      || builtins.match ".*\\.(s|ld)$" path != null;
  };

  commonArgs = {
    pname = "efiks-kernel";
    version = "0.0.0";

    inherit src;

    strictDeps = true;
    doCheck = false;

    CARGO_BUILD_TARGET = "riscv64gc-unknown-none-elf";
    cargoExtraArgs = "-p kernel";
  };

  cargoArtifacts = craneLib.buildDepsOnly commonArgs;
in
craneLib.buildPackage (commonArgs // {
  inherit cargoArtifacts;

  meta = {
    description = "The efiks kernel";
    license = pkgs.lib.licenses.gpl3Only;
    mainProgram = "kernel";
  };
})
