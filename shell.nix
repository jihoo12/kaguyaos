{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  buildInputs = with pkgs; [
    rustup          # nixpkgs 내장 rustup (외부 설치 아님)
    qemu
    OVMF
  ];

  shellHook = ''
    export RUSTUP_HOME="$PWD/.rustup"
    export CARGO_HOME="$PWD/.cargo"
    export PATH="$CARGO_HOME/bin:$PATH"
    export OVMF_BIOS="${pkgs.OVMF.fd}/FV/OVMF.fd"

    # 처음 한 번만 실행
    if [ ! -d "$CARGO_HOME/bin/cargo" ]; then
      rustup target add x86_64-unknown-uefi
    fi

    echo "🦀 UEFI dev environment ready"
  '';
}
