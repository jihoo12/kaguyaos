# Source from a script after changing to the repository root.
if [[ -z "${JIHOO_BIN:-}" ]]; then
    if [[ -d ../jihoo/compiler ]]; then
        compiler_package="$(nix build path:../jihoo#jihoo --no-link --print-out-paths)"
        export JIHOO_BIN="$compiler_package/bin/jihoo"
    else
        export JIHOO_BIN=jihoo
    fi
fi
export CC="${CC:-clang}"
export JIHOO_LD="${JIHOO_LD:-ld.lld}"
