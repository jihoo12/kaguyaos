{
  description = "Rust UEFI development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
      };
    in
    {
      devShells.${system}.default = pkgs.mkShell {
        packages = with pkgs; [
          rustup
          qemu
          OVMF
          python3
          binutils
        ];

        shellHook = ''
          export RUSTUP_HOME="$PWD/.rustup"
          export CARGO_HOME="$PWD/.cargo"
          export PATH="$CARGO_HOME/bin:$PATH"

          export OVMF_BIOS="${pkgs.OVMF.fd}/FV/OVMF.fd"

          # Rust toolchain이 없을 때만 초기화
          if [ ! -x "$CARGO_HOME/bin/cargo" ]; then
            echo "Initializing Rust toolchain..."
            rustup default stable
            rustup target add x86_64-unknown-uefi
          fi

          echo "🦀 UEFI dev environment ready"
        '';
      };
    };
}
