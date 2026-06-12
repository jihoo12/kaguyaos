ENTRY(_start)

SECTIONS
{
    . = 0x0;

    .header : {
        /* Magic: "KEF\0" */
        BYTE(0x4B); BYTE(0x45); BYTE(0x46); BYTE(0x00);
        /* entry_offset: offset of _start from the start of the code segment (0x10) */
        LONG(_start - 0x10);
        /* code_offset: 0x10 */
        LONG(0x10);
        /* code_size */
        LONG(_code_end - 0x10);
    }

    /* Code segment starts at file offset 16 (0x10) */
    . = 0x10;

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
