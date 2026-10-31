# Contextual type through boxed-field assignment

Reported as [jihoo issue #23](https://github.com/jihoolang/jihoo/issues/23)
on freshly built compiler `d0b3d3182a33e602ef687561633d6e2be9d0472b`.

```sh
compiler_package=$(nix build path:../jihoo#jihoo --no-link --print-out-paths)
compiler="$compiler_package/bin/jihoo"
cases=tools/jihoo-regressions/box-field-assignment
"$compiler" check "$cases/box.jh"
"$compiler" check "$cases/vector.jh"
"$compiler" check "$cases/box-uefi.jh"
```

Each check incorrectly rejects `row.value = None` (or
`rows[0].value = None`) with `unknown name None`. The expected field type is
`Option(u64)`. Both native and UEFI checking fail before object emission.
The native cases should run and print `true`.

Run the controls sequentially (compiler `run` uses a shared temporary path):

```sh
"$compiler" run "$cases/local.jh"
"$compiler" run "$cases/explicit.jh"
"$compiler" run "$cases/typed.jh"
"$compiler" run "$cases/pointer.jh"
```

All four controls print `true` and exit 0. They use a local struct, explicit
`deref_mut()`, a separately typed `Option(u64)` RHS, and a raw struct pointer.
The typed-RHS control confirms that automatic boxed-field assignment works;
its contextual RHS type is what gets lost.

The excluded `src/jihoo/task.jh` scheduler draft uses `Vec(Box(Task))` and
fails at `scheduler.tasks[i].address_space = None`. An ordinary untyped
`if` zero branch was separately annotated `u64`; that was a porting error,
not this compiler problem. No typed-RHS or explicit-projection workaround
is connected. Migration is stopped pending a compiler fix. Rebuild the
compiler and rerun these cases before continuing scheduler compilation,
assembly handoffs, syscall integration and QEMU checks.

## Fixed compiler verification

On `8a7aecfcf3b4e72ac40890f739deb96cca3019f6`, all three checks above
pass; `box.jh`, `vector.jh` and the four controls run and print `true`.
The full scheduler draft also passes native checking and object emission.
The scheduler now has native stack-switch tests and a standalone UEFI
integration check; see [the migration notes](../../../docs/jihoo-full-port.md).
