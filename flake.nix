{
  description = "agent-app dev shell: Rust, Tauri 2 build dependencies, system WebKitGTK 4.1";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
    in {
      devShells.${system}.default = pkgs.mkShell {
        nativeBuildInputs = with pkgs; [ cargo rustc gcc pkg-config xvfb-run imagemagick squashfsTools curl ];
        buildInputs = with pkgs; [ webkitgtk_4_1 gtk3 libsoup_3 librsvg openssl glib cairo pango atk gdk-pixbuf harfbuzz ];
      };
    };
}
