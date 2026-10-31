# Nonzero static array initializer regression

Reported as [jihoo issue #22](https://github.com/jihoolang/jihoo/issues/22)
on freshly built compiler `c8014f4b1d3bbc22bfc4c267193b47737268bf1a`.
A static array with a nonzero constant initializer passes checking and
object emission, but its LLVM global is silently zero-initialized.

```sh
compiler_package=$(nix build path:../jihoo#jihoo --no-link --print-out-paths)
compiler="$compiler_package/bin/jihoo"
cases=tools/jihoo-regressions/static-initializer
"$compiler" check "$cases/array.jh"
"$compiler" run -O0 "$cases/array.jh"
"$compiler" run -O2 "$cases/array.jh"
JIHOO_BACKEND=clang JIHOO_CC=clang "$compiler" run -O2 "$cases/array.jh"
"$compiler" run "$cases/repeated.jh"
```

Run these commands sequentially: compiler `run` uses a shared temporary
executable path. The explicit-list case should print `12`, `30`, `51`, `63`
and exit 0; it prints four zeros and exits 1. The repeated-initializer case
should print four `12`s and exit 0; it prints four zeros and exits 1.
Both object backends and optimization levels produce the wrong data.

```sh
"$compiler" run -O0 "$cases/controls.jh"
"$compiler" run -O2 "$cases/controls.jh"
"$compiler" emit-llvm --lib "$cases/values.jh" -o /tmp/kaguya-static-native.ll
"$compiler" emit-llvm --lib "$cases/array-uefi.jh" -o /tmp/kaguya-static-uefi.ll
"$compiler" emit-obj --lib "$cases/values.jh" -o /tmp/kaguya-static-native.o
"$compiler" emit-obj --lib "$cases/array-uefi.jh" -o /tmp/kaguya-static-uefi.obj
```

The static scalar, zeroed static array, const array, and local array controls
print `12`, `0`, `12`, `12` and exit 0. Native and UEFI object emission both
succeed, but both LLVM files contain an incorrect `[4 x i8] zeroinitializer`
for `VALUES`. The native renderer test in `tools/test-jihoo-console.sh`
therefore fails its glyph-pixel assertion: all glyph bytes are zero.

In `compiler/llvm.jh`, the static initializer emission handles scalar
constants explicitly but falls back to zero bytes for other cases,
including `mir.Const.Bytes`. This is the suspected cause, based on code and
IR inspection; no compiler patch or production workaround is applied.
Fixed in `d0b3d3182a33e602ef687561633d6e2be9d0472b`. The explicit-list and repeated
array cases now print their initialized values, at both optimization levels
and with the Clang control. Const/local/scalar/zeroed controls still pass;
UEFI IR contains the four actual bytes and COFF emission succeeds. The
compiler's own aggregate/static regression and the console C harness both
pass. No const-array or runtime-initialization workaround is used.
