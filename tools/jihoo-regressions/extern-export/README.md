# External declaration and exported-definition regression

Reported as [jihoo issue #21](https://github.com/jihoolang/jihoo/issues/21)
on compiler `12f3827bd2574255934b7c730c1746ace14e5197`, freshly built through
Nix. Checking succeeds but LLVM IR contains both a `declare` and a `define`
under the same C function name. Native and UEFI object emission fail with
`invalid redefinition of function 'callback'`, including the Clang backend.

From the repository root:

```sh
compiler_package=$(nix build path:../jihoo#jihoo --no-link --print-out-paths)
compiler="$compiler_package/bin/jihoo"
cases=tools/jihoo-regressions/extern-export
"$compiler" check --lib "$cases/library.jh"
"$compiler" emit-obj --lib "$cases/library.jh" -o /tmp/kaguya-combined.o
"$compiler" emit-obj --lib "$cases/library-uefi.jh" -o /tmp/kaguya-combined.obj
```

On the reported compiler, checking exits 0 while object emission exits 1;
`combined.jh` also checks but fails to build. Emitting LLVM exposes the
duplicate entries:

```sh
"$compiler" emit-llvm --lib "$cases/library.jh" -o /tmp/kaguya-combined.ll
```

Controls:

```sh
"$compiler" emit-obj --lib "$cases/provider.jh" -o /tmp/kaguya-provider.o
"$compiler" run "$cases/separate.jh" /tmp/kaguya-provider.o
"$compiler" run "$cases/ordinary.jh"
```

Each run prints `42` and exits 0. The first control keeps the C ABI but puts
the provider in a separately compiled object; the second uses an ordinary
public jihoo module function. These are diagnostic controls, not workarounds
connected to the standalone kernel build.

## Resolution

Fixed in `c8014f4b1d3bbc22bfc4c267193b47737268bf1a`, freshly built through
Nix. Both object commands now succeed. `combined.jh`, `ordinary.jh`, and
the separate-object control each run and print `42`. The compiler's
`extern_export` execution regression passes, while its signature-mismatch
and duplicate-export failure cases are rejected. The standalone kernel uses
the matching declarations/definitions directly, without a separate-object
workaround.
