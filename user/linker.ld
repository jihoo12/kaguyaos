ENTRY(_start)

/* Programs run at USER_CODE_BASE in their own address space, so link them
 * there: absolute addresses (GOT entries, jump tables) are then correct
 * without runtime relocation. The 16-byte header sits just below it. */
USER_CODE_BASE = 0x400000000000;

SECTIONS
{
    . = USER_CODE_BASE - 0x10;

    .header : {
        /* Magic: "KEF\0" */
        BYTE(0x4B); BYTE(0x45); BYTE(0x46); BYTE(0x00);
        /* entry_offset: offset of _start from the start of the code segment */
        LONG(_start - USER_CODE_BASE);
        /* code_offset: 0x10 */
        LONG(0x10);
        /* code_size */
        LONG(_code_end - USER_CODE_BASE);
    }

    /* Code segment starts at file offset 16 (0x10) */
    . = USER_CODE_BASE;

    .text : {
        _code_start = .;
        *(.text .text.*)
    }

    .rodata : {
        *(.rodata .rodata.*)
    }

    .data : {
        *(.data .data.*)
        /* Merge bss into data so it is zero-filled in the flat binary */
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
