{
  description = "Native code-driven CAD editor";

  inputs.nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
  inputs.occt-nixpkgs.url = "github:nixos/nixpkgs/nixos-24.11";
  inputs.nix-appimage = {
    url = "github:ralismark/nix-appimage/7946addbc0d97e358a6d7aefe5e82310f0fe6b18";
    inputs.nixpkgs.follows = "nixpkgs";
  };
  inputs.smlib = {
    url = "git+https://github.com/awv-informatik/smlibpy?submodules=1&rev=735403a40b261b71967a59d670cd7970f032d04e";
    flake = false;
  };

  outputs =
    {
      self,
      nixpkgs,
      occt-nixpkgs,
      nix-appimage,
      smlib,
      ...
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      for_each_system = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = for_each_system (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          version = (builtins.fromTOML (builtins.readFile ./crates/spice-cad/Cargo.toml)).package.version;
        in
        {
          default = self.packages.${system}.spice-cad;
          spice-cad-appimage = nix-appimage.lib.${system}.mkAppImage {
            program = "${self.packages.${system}.spice-cad}/bin/spice-cad";
            name = "spice-cad-${version}-${system}.AppImage";
          };
          spice-cad = pkgs.rustPlatform.buildRustPackage {
            pname = "spice-cad";
            inherit version;
            src = pkgs.lib.cleanSourceWith {
              src = ./.;
              filter =
                path: _type:
                let
                  relative = pkgs.lib.removePrefix "${toString ./.}/" (toString path);
                  root = builtins.head (pkgs.lib.splitString "/" relative);
                in
                builtins.elem root [
                  "Cargo.toml"
                  "Cargo.lock"
                  "LICENSE"
                  "NOTICE"
                  "ASSETS.md"
                  "about.toml"
                  "licenses"
                  "crates"
                  "tools"
                  "examples"
                ];
            };
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildFlags = [
              "-p"
              "spice-cad"
            ];
            doCheck = false;
            nativeBuildInputs = [
              pkgs.pkg-config
              pkgs.makeWrapper
            ];
            buildInputs = [
              self.packages.${system}.smlib
              pkgs.tbb
            ];
            SMLIB_ROOT = self.packages.${system}.smlib;
            postInstall = ''
              wrapProgram $out/bin/spice-cad \
                --prefix PATH : ${
                  pkgs.lib.makeBinPath [
                    self.packages.${system}.luau
                    pkgs.luau-lsp
                  ]
                } \
                --prefix LD_LIBRARY_PATH : ${
                  pkgs.lib.makeLibraryPath [
                    pkgs.libxkbcommon
                    pkgs.wayland
                    pkgs.libGL
                    pkgs.vulkan-loader
                    pkgs.libx11
                    pkgs.libxcursor
                    pkgs.libxi
                    pkgs.libxrandr
                  ]
                }
              install -Dm644 ${pkgs.writeText "spice-cad.desktop" ''
                [Desktop Entry]
                Type=Application
                Name=Spice CAD
                Icon=spice-cad
                Exec=spice-cad
                Terminal=false
                Categories=Graphics;3DGraphics;Engineering;
              ''} $out/share/applications/spice-cad.desktop
              install -Dm644 crates/spice-cad/assets/spice-cad.svg \
                $out/share/icons/hicolor/scalable/apps/spice-cad.svg
              install -Dm644 LICENSE $out/share/doc/spice-cad/LICENSE
              install -m644 NOTICE ASSETS.md $out/share/doc/spice-cad/
              cp -r licenses $out/share/doc/spice-cad/licenses
              install -Dm644 crates/spice-cad/assets/LICENSE \
                $out/share/doc/spice-cad/crates/spice-cad/assets/LICENSE
            '';
            meta = {
              description = "Native code-first CAD editor with typed Luau and SMLib";
              mainProgram = "spice-cad";
              license = pkgs.lib.licenses.mit;
              platforms = systems;
            };
          };
          luau = pkgs.luau.overrideAttrs {
            version = "0.740";
            nativeBuildInputs = [
              pkgs.cmake
              pkgs.python3
            ];
            src = pkgs.fetchFromGitHub {
              owner = "luau-lang";
              repo = "luau";
              rev = "0.740";
              hash = "sha256-rb0BpRsAyN37ZlrkLpaiyk3yfoIQDU7YnXpkb8vEyzY=";
            };
          };
          smlib = pkgs.stdenv.mkDerivation {
            pname = "smlib-native";
            version = "735403a";
            src = smlib;
            nativeBuildInputs = [
              pkgs.cmake
              pkgs.ninja
            ];
            buildInputs = [ pkgs.tbb ];
            cmakeFlags = [
              "-DSMLIBPY_KERNEL_ONLY=ON"
              "-DSMLIBPY_USE_TBB=ON"
              "-DCMAKE_BUILD_TYPE=Release"
            ];
            enableParallelBuilding = true;
            installPhase = ''
              mkdir -p $out/lib $out/include
              cp libsmlib.a libsm_api.a $out/lib/
              cp -r ../third_party/usd-brep/source/SMLib/inc/. $out/include/
              cp -r ../third_party/usd-brep/source/SM_API/inc/. $out/include/
            '';
          };
        }
      );
      devShells = for_each_system (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          occt_pkgs = import occt-nixpkgs { inherit system; };
          occt = occt_pkgs.opencascade-occt;
        in
        {
          format = pkgs.mkShellNoCC {
            packages = with pkgs; [
              cargo
              rustfmt
              nixfmt
              actionlint
              yamlfmt
            ];
          };
          ci = self.devShells.${system}.default.overrideAttrs (previous: {
            nativeBuildInputs = previous.nativeBuildInputs ++ [ pkgs.lld ];
            LD_LIBRARY_PATH =
              previous.LD_LIBRARY_PATH
              + ":${
                pkgs.lib.makeLibraryPath [
                  pkgs.stdenv.cc.cc.lib
                ]
              }";
          });
          verification = self.devShells.${system}.ci.overrideAttrs (previous: {
            nativeBuildInputs = previous.nativeBuildInputs ++ [ pkgs.cmake ];
            buildInputs = previous.buildInputs ++ [ occt ];
            LD_LIBRARY_PATH = previous.LD_LIBRARY_PATH + ":${pkgs.lib.makeLibraryPath [ occt ]}";
            DEP_OCCT_ROOT = "${occt}";
            CMAKE_PREFIX_PATH = "${occt}";
            CMAKE_POLICY_VERSION_MINIMUM = "3.10";
          });
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              cargo-about
              rustc
              rustfmt
              clippy
              pkg-config
              git
              ripgrep
              nixfmt
              self.packages.${system}.luau
              luau-lsp
              stylua
            ];
            buildInputs = [
              self.packages.${system}.smlib
              pkgs.tbb
              pkgs.libxkbcommon
              pkgs.wayland
              pkgs.libGL
              pkgs.vulkan-loader
              pkgs.libx11
              pkgs.libxcursor
              pkgs.libxi
              pkgs.libxrandr
            ];
            SMLIB_ROOT = self.packages.${system}.smlib;
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
              pkgs.tbb
              pkgs.libxkbcommon
              pkgs.wayland
              pkgs.libGL
              pkgs.vulkan-loader
              pkgs.libx11
              pkgs.libxcursor
              pkgs.libxi
              pkgs.libxrandr
            ];
          };
        }
      );
      formatter = for_each_system (system: nixpkgs.legacyPackages.${system}.nixfmt);
    };
}
