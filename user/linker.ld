ENTRY(_start)

/* Programs run at USER_CODE_BASE in their own address space, so link them
 * there: absolute addresses (GOT entries, jump tables) are then correct
 * without runtime relocation. The 32-byte header sits just below it. */
USER_CODE_BASE = 0x400000000000;
KEF_HEADER_SIZE = 0x20;

SECTIONS
{
    . = USER_CODE_BASE - KEF_HEADER_SIZE;

    .header : {
        /* Magic: "KEF" + version 1 */
        BYTE(0x4B); BYTE(0x45); BYTE(0x46); BYTE(0x01);
        /* entry_offset: offset of _start from the start of the image */
        LONG(_start - USER_CODE_BASE);
        /* code_offset: file offset of the image */
        LONG(KEF_HEADER_SIZE);
        /* code_size: in-memory image size, including .bss */
        LONG(_code_end - USER_CODE_BASE);
        /* init_size: bytes stored in the file; the rest is zero-filled */
        LONG(_init_end - USER_CODE_BASE);
        LONG(0); LONG(0); LONG(0);
    }

    . = USER_CODE_BASE;

    .text : {
        _code_start = .;
        *(.text .text.*)
    }

    .rodata : {
        *(.rodata .rodata.*)
    }

    .got : { *(.got) *(.got.plt) }

    .data : {
        *(.data .data.*)
    }

    /* Everything before .bss, including orphan sections such as .dynamic,
     * is stored in the file. */
    .bss : {
        _init_end = .;
        *(.bss .bss.*)
        *(COMMON)
    }

    _code_end = .;

    /DISCARD/ : {
        *(.comment)
        *(.eh_frame)
        *(.note.gnu.property)
    }
}
