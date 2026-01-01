{
  description = "A simple 'lights out' generator and solver";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
    flake-parts.url = "github:hercules-ci/flake-parts";
    treefmt-nix.url = "github:numtide/treefmt-nix";
    systems.url = "github:nix-systems/default";
  };

  outputs =
    inputs:
    inputs.flake-parts.lib.mkFlake { inherit inputs; } {
      systems = import inputs.systems;
      imports = [ inputs.treefmt-nix.flakeModule ];

      perSystem =
        {
          self',
          pkgs,
          system,
          ...
        }:
        let
          fenixPkgs = inputs.fenix.packages.${system};

          toolchain = fenixPkgs.combine [
            fenixPkgs.stable.rustc
            fenixPkgs.stable.cargo
            fenixPkgs.stable.rustfmt
            fenixPkgs.stable.clippy
            fenixPkgs.stable.rust-analyzer
            fenixPkgs.stable.rust-src
            fenixPkgs.targets.wasm32-unknown-unknown.stable.rust-std
          ];

          craneLib = (inputs.crane.mkLib pkgs).overrideToolchain toolchain;

          src = craneLib.cleanCargoSource ./.;

          commonArgs = {
            inherit src;
            pname = "lights-out";
            # Native checks cover the wasm crate too, as `wasm-bindgen` builds on the host.
            cargoExtraArgs = "--workspace";
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          # Separate dependency cache for the wasm target.
          # Host-target artifacts can't be reused for wasm32-unknown-unknown.
          wasmArgs = {
            inherit src;
            pname = "lights-out-wasm";
            cargoExtraArgs = "-p lights-out-wasm --target wasm32-unknown-unknown";
            doCheck = false;
          };
          wasmCargoArtifacts = craneLib.buildDepsOnly wasmArgs;
          wasmCrate = craneLib.buildPackage (wasmArgs // { cargoArtifacts = wasmCargoArtifacts; });

          # Everything the blog serves, laid out like the Hugo site so it can be copied over as is.
          # Scripts are static files, while the stylesheet goes through Hugo's `assets/` pipeline.
          web =
            pkgs.runCommandLocal "lights-out-web"
              {
                nativeBuildInputs = [
                  pkgs.wasm-bindgen-cli
                  pkgs.binaryen
                  pkgs.typescript
                ];
              }
              ''
                js=$out/static/js/lights-out
                mkdir -p $js/pkg $out/assets/css

                # wasm-bindgen JS glue + optimised wasm.
                wasm-bindgen \
                  --target web \
                  --out-dir pkg \
                  --out-name lights_out_wasm \
                  ${wasmCrate}/lib/lights_out_wasm.wasm
                wasm-opt -Oz -o $js/pkg/lights_out_wasm_bg.wasm pkg/lights_out_wasm_bg.wasm
                cp pkg/lights_out_wasm.js $js/pkg/

                # `index.ts` imports `./pkg/lights_out_wasm.js`, typed by the `.d.ts` that wasm-bindgen writes beside it.
                cp ${./web}/index.ts ${./web}/tsconfig.json .
                tsc -p tsconfig.json
                cp index.js $js/

                cp ${./web}/lights-out.css $out/assets/css/
              '';
        in
        {
          packages = {
            inherit web;
            wasm = wasmCrate;
            default = web;
          };

          checks = {
            clippy = craneLib.cargoClippy (
              commonArgs
              // {
                inherit cargoArtifacts;
                cargoClippyExtraArgs = "--all-targets -- --deny warnings";
              }
            );

            tests = craneLib.cargoTest (commonArgs // { inherit cargoArtifacts; });

            # Builds the wasm and type-checks the front end.
            inherit web;
          };

          treefmt.config = {
            projectRootFile = "flake.nix";
            programs.rustfmt.enable = true;
            programs.nixfmt.enable = true;
          };

          devShells.default = craneLib.devShell {
            checks = self'.checks;
            packages = with pkgs; [
              typescript
              wasm-bindgen-cli
              binaryen
              lldb
            ];
          };
        };
    };
}
