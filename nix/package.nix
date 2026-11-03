{ lib, rustPlatform, buildNpmPackage, importNpmLock, nodejs_22 }:
let
  # Explicit source lists keep captures, credentials and local build artifacts out of the store.
  frontendSource = lib.fileset.toSource {
    root = ../web;
    fileset = lib.fileset.unions [
      ../web/src ../web/index.html ../web/package.json ../web/package-lock.json
      ../web/tsconfig.json ../web/vite.config.ts
    ];
  };
  frontend = buildNpmPackage {
    pname = "smartthermweb-ui";
    version = "0.1.0";
    src = frontendSource;
    nodejs = nodejs_22;
    npmDeps = importNpmLock { npmRoot = frontendSource; };
    npmConfigHook = importNpmLock.npmConfigHook;
    doCheck = true;
    checkPhase = ''
      runHook preCheck
      npm run check
      npm test
      runHook postCheck
    '';
    installPhase = ''
      runHook preInstall
      mkdir -p $out
      cp -r dist/. $out/
      runHook postInstall
    '';
  };
in rustPlatform.buildRustPackage {
  pname = "smartthermweb";
  version = "0.1.0";
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../src ../tests ../Cargo.toml ../Cargo.lock ../build.rs
    ];
  };
  cargoLock.lockFile = ../Cargo.lock;
  SMARTTHERM_REQUIRE_WEB = "1";
  preBuild = ''
    mkdir -p web/dist
    cp -r ${frontend}/. web/dist/
  '';
  doCheck = true;
  meta = {
    description = "Self-hosted OCSTAT thermostat dashboard";
    license = lib.licenses.asl20;
    platforms = lib.platforms.linux;
    mainProgram = "smartthermweb";
  };
}
