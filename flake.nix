{
  description = "Native Rust CAD editor and parametric spice rack";

  inputs.nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
  inputs.occt-nixpkgs.url = "github:nixos/nixpkgs/nixos-24.11";
  inputs.smlib = {
    url = "git+https://github.com/awv-informatik/smlibpy?submodules=1&rev=735403a40b261b71967a59d670cd7970f032d04e";
    flake = false;
  };

  outputs =
    {
      self,
      nixpkgs,
      occt-nixpkgs,
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
          occt_pkgs = import occt-nixpkgs { inherit system; };
        in
        {
          default = self.packages.${system}.spice-cad;
          spice-cad = pkgs.rustPlatform.buildRustPackage {
            pname = "spice-cad";
            version = "0.1.0";
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
                  "native"
                  "src"
                  "models"
                ];
            };
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildFlags = [
              "-p"
              "spice-cad"
            ];
            doCheck = false;
            nativeBuildInputs = [
              pkgs.cmake
              pkgs.pkg-config
              pkgs.makeWrapper
            ];
            buildInputs = [
              self.packages.${system}.smlib
              pkgs.tbb
            ];
            SMLIB_ROOT = self.packages.${system}.smlib;
            CMAKE_POLICY_VERSION_MINIMUM = "3.10";
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
              install -Dm644 native/assets/spice-cad.svg \
                $out/share/icons/hicolor/scalable/apps/spice-cad.svg
            '';
            meta = {
              description = "Native code-first CAD editor with typed Luau and SMLib";
              mainProgram = "spice-cad";
              platforms = systems;
            };
          };
          spice-cad-rack-tools = self.packages.${system}.spice-cad.overrideAttrs (previous: {
            pname = "spice-cad-rack-tools";
            cargoBuildFlags = previous.cargoBuildFlags ++ [
              "--features"
              "rack-tools"
            ];
            buildInputs = previous.buildInputs ++ [ occt_pkgs.opencascade-occt ];
            DEP_OCCT_ROOT = occt_pkgs.opencascade-occt;
          });
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
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              rustc
              rustfmt
              clippy
              cmake
              pkg-config
              git
              ripgrep
              nixfmt
              self.packages.${system}.luau
              luau-lsp
              stylua
            ];
            buildInputs = [
              occt
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
            DEP_OCCT_ROOT = "${occt}";
            CMAKE_PREFIX_PATH = "${occt}";
            CMAKE_POLICY_VERSION_MINIMUM = "3.10";
          };
        }
      );
      formatter = for_each_system (system: nixpkgs.legacyPackages.${system}.nixfmt);
    };
}
