#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
output="$PWD/target/rust-free-check"
mkdir -p "$output/guard"
export RUST_FORBIDDEN_LOG="$output/forbidden.log"
: > "$RUST_FORBIDDEN_LOG"
for tool in cargo rustc rustup rust-lld; do
    cat > "$output/guard/$tool" <<'GUARD'
#!/usr/bin/env bash
printf '%s\n' "$0" >> "$RUST_FORBIDDEN_LOG"
echo "Rust tool invoked during Jihoo build: $0" >&2
exit 97
GUARD
    chmod +x "$output/guard/$tool"
done
export PATH="$output/guard:$PATH"
source tools/jihoo-env.sh
bash user/build.sh
KAGUYA_DISK_IMAGE="$output/nvme.img" bash user/insert.sh
bash build.sh
bash tools/build-hardware-test.sh
bash tools/build-hardware-shell.sh
# An existing unrecognized image must fail without changing even one byte.
printf 'unrecognized image: preserve these bytes\n' > "$output/invalid.img"
cp "$output/invalid.img" "$output/invalid.before"
if bash tools/prepare-image.sh "$output/invalid.img" > "$output/invalid.log" 2>&1; then
    echo 'Unrecognized image was unexpectedly accepted' >&2; exit 1
fi
cmp "$output/invalid.img" "$output/invalid.before"
if [[ -s "$RUST_FORBIDDEN_LOG" ]]; then cat "$RUST_FORBIDDEN_LOG" >&2; exit 1; fi
printf '%s\n' 'Rust-free build passed: kernel, seven programs, host image tool, both hardware images; no Rust tools invoked.'
