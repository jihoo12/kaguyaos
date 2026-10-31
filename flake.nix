{
  description = "Jihoo x86_64 UEFI development environment";

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
          qemu
          OVMF
          python3
          binutils
          llvmPackages_21.clang
          llvmPackages_21.lld
        ];

        shellHook = ''
          export OVMF_BIOS="${pkgs.OVMF.fd}/FV/OVMF.fd"
          export CC=clang
          export JIHOO_LD=ld.lld
          echo "Jihoo UEFI environment ready; compiler source: ../jihoo"
        '';
      };
    };
}
