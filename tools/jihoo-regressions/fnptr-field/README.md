# Raw-pointer function-field call regression

Reported as [jihoo issue #20](https://github.com/jihoolang/jihoo/issues/20)
on compiler commit `84d4a54a0de4a8c12f7584ee390e64083ac3a8b8`.
The compiler was built from that checkout with Nix, without using `out/jihoo`.

```sh
compiler_package=$(nix build path:../jihoo#jihoo --no-link --print-out-paths)
compiler="$compiler_package/bin/jihoo"
"$compiler" check --lib tools/jihoo-regressions/fnptr-field/pointer.jh
```

The last command currently exits 1 with `` `Callbacks` has no method `call` ``.
It should accept the declared `fnptr` field through the raw pointer.
The same reproduction prefixed with `#![uefi]` also fails during checking;
LLVM, linking, kernel register options, and QEMU are not involved.

The controls check successfully and each prints `42` when run:

```sh
for control in borrowed local dereferenced; do
    "$compiler" check --lib "tools/jihoo-regressions/fnptr-field/$control.jh"
    "$compiler" run "tools/jihoo-regressions/fnptr-field/$control.jh"
done
```

| Source | Receiver/callee | Check on `84d4a54` | Run |
|--------|-----------------|---------------------|-----|
| `pointer.jh` | `callbacks.call(value)` through `*Callbacks` | Exit 1 | Not built |
| `borrowed.jh` | Same field call on a borrowed struct | Exit 0 | `42` |
| `local.jh` | Load `callbacks.call` through the pointer, then call the local | Exit 0 | `42` |
| `dereferenced.jh` | `(*callbacks).call(value)` | Exit 0 | `42` |

These controls isolate field-call resolution from field projection, pointer
representation, and the function-pointer calling convention. They are
diagnostic cases, not workarounds connected to the kernel. Once the compiler
is fixed, the pointer case should pass `check --lib` too.
