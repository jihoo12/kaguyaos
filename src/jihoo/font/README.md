# Basic Latin bitmap font

`basic.jh` contains the 128 eight-byte Basic Latin glyphs from
`font8x8` 0.3.1 (`src/legacy.rs`, `BASIC_LEGACY`), previously used by the
Rust console. Bits run from left to right, with eight rows per glyph.
The data is vendored so standalone jihoo builds do not need Cargo or a
Rust font crate. See [LICENSE](LICENSE) for the upstream MIT license.

Compiler [issue #22](https://github.com/jihoolang/jihoo/issues/22) is fixed
in `d0b3d31`. The native renderer tests at `-O0` and `-O2` and standalone
UEFI/QEMU text output now pass with this static initializer.
