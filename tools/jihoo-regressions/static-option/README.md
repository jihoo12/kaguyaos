# Static Option initializer rejects None

Reported as [jihoo issue #24](https://github.com/jihoolang/jihoo/issues/24)
on compiler `8a7aecfcf3b4e72ac40890f739deb96cca3019f6` (`jihoo 0.4.2`).

```sh
compiler_package=$(nix build path:../jihoo#jihoo --no-link --print-out-paths)
compiler="$compiler_package/bin/jihoo"
cases=tools/jihoo-regressions/static-option
"$compiler" check "$cases/none.jh"
"$compiler" check "$cases/none-uefi.jh"
```

Both incorrectly reject `static VALUE: Option(u64) = None` with
`` `None` is not a compile-time constant ``. The native case should run and
print `true`; the UEFI case should return 0. The excluded graphics draft
fails on the same initializer for `Option((u64, u64, u64, u64))`.

The controls check and run successfully at both optimization levels:

```sh
for level in -O0 -O2; do
    "$compiler" run "$level" "$cases/local.jh"
    "$compiler" run "$level" "$cases/function.jh"
    "$compiler" run "$level" "$cases/some.jh"
    "$compiler" run "$level" "$cases/qualified.jh"
done
"$compiler" emit-obj "$cases/function-uefi.jh" -o /tmp/kaguya-static-option-control.obj
```

Each native run prints `true` and exits 0. The UEFI function-returned
initializer emits COFF successfully. Run controls sequentially because
the compiler's `run` command uses a shared temporary executable path.

Local `None`, a function returning `None`, direct `Some(7)` and a qualified
custom enum variant establish that the expected type, value and static
enum storage work. The contextual nullary name takes a different constant
evaluation path. Source inspection suggests the `Name` branch of
`Sema.const_expr` lacks the `eval_const_expr` fallback used by its `Field`
branch; that is an inference, not a verified compiler fix.

No compiler patch or production initializer workaround is connected.
Migration is stopped as requested; rebuild the compiler and rerun these
cases before continuing the graphics draft.
