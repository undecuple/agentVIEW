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
      fonts-conf = pkgs.makeFontsConf { fontDirectories = [ pkgs.dejavu_fonts ]; };
      standalone-extras = pkgs.symlinkJoin {
        name = "agentview-standalone-extras";
        paths = with pkgs; [ dejavu_fonts fontconfig shared-mime-info gsettings-desktop-schemas adwaita-icon-theme gdk-pixbuf librsvg bubblewrap glib ];
      };
      };
      devShells.${system} = {
        default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [ cargo rustc gcc pkg-config python3 xvfb-run imagemagick squashfsTools curl ];
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
