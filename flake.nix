{
  description = "Resonance - A Rust Discord music bot";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        rustToolchain = pkgs.rust-bin.stable."1.91.0".default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = [
            rustToolchain

            # Build dependencies for songbird (opus codec)
            pkgs.cmake
            pkgs.pkg-config
            pkgs.libopus

            # Runtime dependencies
            pkgs.yt-dlp
            pkgs.ffmpeg

            # Redis (for local development)
            pkgs.redis

            # Docker
            pkgs.docker
          ];

          PKG_CONFIG_PATH = "${pkgs.libopus}/lib/pkgconfig";

          # Fix Docker DNS resolution on NixOS
          DOCKER_BUILDKIT = "1";

          shellHook = ''
            # Ensure Docker can resolve DNS inside containers
            if [ ! -f /etc/docker/daemon.json ] || ! grep -q "dns" /etc/docker/daemon.json 2>/dev/null; then
              echo "NOTE: If Docker builds fail with DNS errors, run:"
              echo "  sudo mkdir -p /etc/docker"
              echo '  echo '"'"'{"dns":["8.8.8.8","8.8.4.4"]}'"'"' | sudo tee /etc/docker/daemon.json'
              echo "  sudo systemctl restart docker"
              echo ""
            fi

            echo "Resonance dev shell"
            echo "  Rust: $(rustc --version)"
            echo "  yt-dlp: $(yt-dlp --version)"
            echo ""
            echo "  cargo run --release    Run the bot"
            echo "  redis-server           Start Redis locally"
            echo "  docker compose up -d   Run with Docker + Redis"
            echo ""
          '';
        };
      }
    );
}
