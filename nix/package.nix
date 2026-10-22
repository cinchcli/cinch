{
  lib,
  stdenv,
  rustPlatform,
  pkg-config,
  protobuf,
  installShellFiles,
  dbus,
}:

let
  root = ../.;
  manifest = (lib.importTOML ../crates/cli/Cargo.toml).package;
in
rustPlatform.buildRustPackage {
  pname = "cinch";
  inherit (manifest) version;

  # Cargo loads every workspace member's manifest, so the desktop crate has to be
  # present even though only the CLI is built.
  src = lib.fileset.toSource {
    inherit root;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../crates
      (lib.fileset.difference ../apps/desktop/src-tauri (
        lib.fileset.maybeMissing ../apps/desktop/src-tauri/target
      ))
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  cargoBuildFlags = [
    "--package"
    "cinch-cli"
  ];
  cargoTestFlags = [
    "--package"
    "cinch-cli"
  ];

  # protoc for prost-build in crates/client-core/build.rs.
  nativeBuildInputs = [
    pkg-config
    protobuf
    installShellFiles
  ];

  # keyring's Secret Service backend links libdbus on Linux.
  buildInputs = lib.optionals stdenv.hostPlatform.isLinux [ dbus ];

  # `ci` is the short alias the Homebrew formula and cask also install; the CLI
  # dispatches on argv[0] and renders completions keyed to the invoked name.
  postInstall = ''
    ln -s cinch $out/bin/ci
  ''
  + lib.optionalString (stdenv.buildPlatform.canExecute stdenv.hostPlatform) ''
    for name in cinch ci; do
      installShellCompletion --cmd $name \
        --bash <($out/bin/$name completion bash) \
        --zsh <($out/bin/$name completion zsh) \
        --fish <($out/bin/$name completion fish)
    done
  '';

  meta = {
    description = manifest.description;
    homepage = "https://cinchcli.com";
    changelog = "https://github.com/cinchcli/cinch/blob/main/CHANGELOG.md";
    license = lib.licenses.agpl3Only;
    mainProgram = "cinch";
    platforms = lib.platforms.linux ++ lib.platforms.darwin;
  };
}
