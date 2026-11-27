{
  description = "agent-app dev shell: Rust, Tauri 2 build dependencies, system WebKitGTK 4.1";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
    in {
      # Data the standalone AppImage carries besides the binary's own libraries (fonts, icons, schemas, MIME data, bwrap).
      packages.${system} = {
      # Native NixOS build: the app runs directly on the host (no AppImage, no bubblewrap), so the sessions it starts
      # see the system exactly like a terminal does (ssh, sshfs, setuid helpers). Install with `nix profile install`
      # or add `inputs.agentview.packages.x86_64-linux.default` to home.packages.
      default = self.packages.${system}.agentview;
      agentview = pkgs.rustPlatform.buildRustPackage {
        pname = "agentview";
        version = "0.1.0";
        src = pkgs.lib.fileset.toSource {
          root = ./.;
          fileset = pkgs.lib.fileset.unions [ ./Cargo.toml ./Cargo.lock ./core ./sessions ./workspace ./src-tauri ./ui ./dist ./packaging/agent-app.desktop ];
        };
        cargoLock.lockFile = ./Cargo.lock;
        buildAndTestSubdir = "src-tauri";
        nativeBuildInputs = with pkgs; [ pkg-config wrapGAppsHook3 ];
        buildInputs = with pkgs; [ webkitgtk_4_1 gtk3 libsoup_3 librsvg openssl glib cairo pango atk gdk-pixbuf harfbuzz glib-networking gsettings-desktop-schemas sqlite ];
        doCheck = false; # the repo's own scripts run the tests; they need a display and fixtures
        postInstall = ''
          install -Dm644 packaging/agent-app.desktop $out/share/applications/agentview.desktop
          install -Dm644 src-tauri/icons/icon.png $out/share/icons/hicolor/512x512/apps/agent-app.png
          ln -s agent-app $out/bin/agentview
        '';
        meta.mainProgram = "agent-app";
      };
      fonts-conf = pkgs.makeFontsConf { fontDirectories = [ pkgs.dejavu_fonts ]; };
      standalone-extras = pkgs.symlinkJoin {
        name = "agentview-standalone-extras";
        paths = with pkgs; [ dejavu_fonts fontconfig shared-mime-info gsettings-desktop-schemas adwaita-icon-theme gdk-pixbuf librsvg bubblewrap glib ];
      };
      };
      devShells.${system} = {
        default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [ cargo rustc gcc pkg-config python3 xvfb-run xwininfo imagemagick squashfsTools curl ];
          buildInputs = with pkgs; [ webkitgtk_4_1 gtk3 libsoup_3 librsvg openssl glib cairo pango atk gdk-pixbuf harfbuzz ];
        };
        # Optional isolated Wayland/Niri testing; the user's compositor is never reconfigured.
        niri = pkgs.mkShell {
          inputsFrom = [ self.devShells.${system}.default ];
          nativeBuildInputs = with pkgs; [ niri dbus ];
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (with pkgs; [ libx11 libxcursor libxi libxrandr libxcb libxkbcommon wayland libglvnd ]);
        };
      };
    };
}
