{
  description = "Hakobi self-hosted DevOps control plane";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        lib = pkgs.lib;
        source = lib.cleanSourceWith {
          src = ./.;
          filter = path: _type:
            let
              name = baseNameOf path;
            in
            !(builtins.elem name [ ".env" ".git" "node_modules" "target" ])
            && !(lib.hasPrefix "result" name);
        };
        package = pkgs.rustPlatform.buildRustPackage {
          pname = "hakobi";
          version = "0.1.0";
          src = source;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [ pkgs.makeWrapper ];
          postInstall = ''
            wrapProgram $out/bin/hakobi \
              --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.cloudflared ]}
          '';
          meta = {
            description = "Self-hosted DevOps control plane";
            mainProgram = "hakobi";
          };
        };
      in
      {
        packages.default = package;
        apps.default = {
          type = "app";
          program = "${package}/bin/hakobi";
        };
        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            cargo-audit
            cargo-deny
            clippy
            cloudflared
            nodejs_22
            python3
            rust-analyzer
            rustc
            rustfmt
          ];
        };
        checks.default = package;
      });
}
