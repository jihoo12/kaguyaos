#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ -z "${JIHOO_BIN:-}" ]]; then
    compiler_package="$(nix build path:../jihoo#jihoo --no-link --print-out-paths)"
    JIHOO_BIN="$compiler_package/bin/jihoo"
fi
output="$PWD/target/jihoo-acpi-test"
mkdir -p "$output"
for optimization in 0 2; do
    "$JIHOO_BIN" emit-obj --lib "-O$optimization" -I src/jihoo tools/jihoo-acpi-root.jh -o "$output/acpi.o"
    "${CC:-cc}" -Wall -Wextra -Werror tools/jihoo-acpi-test.c "$output/acpi.o" -o "$output/acpi-test"
    "$output/acpi-test"
done
