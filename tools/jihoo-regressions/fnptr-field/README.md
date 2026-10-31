# Raw-pointer function-field call regression

Reported as [jihoo issue #20](https://github.com/jihoolang/jihoo/issues/20)
on compiler commit `84d4a54a0de4a8c12f7584ee390e64083ac3a8b8`.
The compiler was built from that checkout with Nix, without using `out/jihoo`.

```sh
compiler_package=$(nix build path:../jihoo#jihoo --no-link --print-out-paths)
compiler="$compiler_package/bin/jihoo"
"$compiler" check --lib tools/jihoo-regressions/fnptr-field/pointer.jh
```

On `84d4a54`, the last command exited 1 with
`` `Callbacks` has no method `call` ``. The same reproduction prefixed with
`#![uefi]` failed during checking too; LLVM, linking, kernel register options,
and QEMU were not involved.

Compiler commit `12f3827bd2574255934b7c730c1746ace14e5197` fixes the issue.
The pointer case now passes `check --lib` in both profiles and produces `42`
when run with the same local `increment` callback as the controls. The
compiler's own `fnptr_field_call.jh` test passes too; its pointer call outside
`unsafe` is still rejected. No workaround is used in the kernel port.

The controls check successfully and each prints `42` when run:

```sh
for control in borrowed local dereferenced; do
    "$compiler" check --lib "tools/jihoo-regressions/fnptr-field/$control.jh"
    "$compiler" run "tools/jihoo-regressions/fnptr-field/$control.jh"
done
```

| Source | Receiver/callee | Check on `84d4a54` | Run on `84d4a54` |
|--------|-----------------|---------------------|-----|
| `pointer.jh` | `callbacks.call(value)` through `*Callbacks` | Exit 1 | Not built |
| `borrowed.jh` | Same field call on a borrowed struct | Exit 0 | `42` |
| `local.jh` | Load `callbacks.call` through the pointer, then call the local | Exit 0 | `42` |
| `dereferenced.jh` | `(*callbacks).call(value)` | Exit 0 | `42` |

These controls isolate field-call resolution from field projection, pointer
representation, and the function-pointer calling convention. They are
diagnostic cases, not workarounds connected to the kernel. All four check
successfully on `12f3827`.
