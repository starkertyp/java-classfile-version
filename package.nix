{ rustPlatform }:
let toml = (builtins.fromTOML (builtins.readFile ./Cargo.toml));
in rustPlatform.buildRustPackage {
  pname = toml.package.name;
  version = toml.package.version;

  src = ./.;

  cargoLock.lockFile = ./Cargo.lock;

  buildInputs = [ ];
}

