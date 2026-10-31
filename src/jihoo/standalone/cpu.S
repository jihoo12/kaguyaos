.text
.globl jihoo_load_gdt
jihoo_load_gdt:
    lgdt (%rcx)
    pushq $8
    leaq 1f(%rip), %rax
    pushq %rax
    lretq
1:
    movw $16, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %fs
    movw %ax, %gs
    movw %ax, %ss
    movw $40, %ax
    ltr %ax
    ret

.globl jihoo_load_idt
jihoo_load_idt:
    lidt (%rcx)
    ret
.globl jihoo_read_cr2
jihoo_read_cr2:
    movq %cr2, %rax
    ret
.globl jihoo_read_cr3
jihoo_read_cr3:
    movq %cr3, %rax
    ret
.globl jihoo_write_cr3
jihoo_write_cr3:
    movq %rcx, %cr3
    ret
.globl jihoo_breakpoint
jihoo_breakpoint:
    int3
    ret
.globl jihoo_wait_interrupt
jihoo_wait_interrupt:
    sti
    hlt
    cli
    ret

.macro VECTOR n, error=0
.globl jihoo_vector_\n
jihoo_vector_\n:
    .if !\error
    pushq $0
    .endif
    pushq $\n
    jmp jihoo_interrupt_common
.endm

jihoo_interrupt_common:
    testb $3, 24(%rsp)
    jz 1f
    swapgs
1:
    pushq %rax
    pushq %rbx
    pushq %rcx
    pushq %rdx
    pushq %rbp
    pushq %rdi
    pushq %rsi
    pushq %r8
    pushq %r9
    pushq %r10
    pushq %r11
    pushq %r12
    pushq %r13
    pushq %r14
    pushq %r15
    cld
    movq %rsp, %rcx
    andq $-16, %rsp
    subq $48, %rsp
    movq %rcx, 32(%rsp)
    call jihoo_interrupt_dispatch
    movq 32(%rsp), %rsp
    popq %r15
    popq %r14
    popq %r13
    popq %r12
    popq %r11
    popq %r10
    popq %r9
    popq %r8
    popq %rsi
    popq %rdi
    popq %rbp
    popq %rdx
    popq %rcx
    popq %rbx
    popq %rax
    addq $16, %rsp
    testb $3, 8(%rsp)
    jz 2f
    swapgs
2:
    iretq

.globl jihoo_vector_address
jihoo_vector_address:
    cmpq $255, %rcx
    ja 1f
    leaq jihoo_vector_table(%rip), %rax
    movq (%rax,%rcx,8), %rax
    ret
1:
    xorq %rax, %rax
    ret

VECTOR 0, 0
VECTOR 1, 0
VECTOR 2, 0
VECTOR 3, 0
VECTOR 4, 0
VECTOR 5, 0
VECTOR 6, 0
VECTOR 7, 0
VECTOR 8, 1
VECTOR 9, 0
VECTOR 10, 1
VECTOR 11, 1
VECTOR 12, 1
VECTOR 13, 1
VECTOR 14, 1
VECTOR 15, 0
VECTOR 16, 0
VECTOR 17, 1
VECTOR 18, 0
VECTOR 19, 0
VECTOR 20, 0
VECTOR 21, 1
VECTOR 22, 0
VECTOR 23, 0
VECTOR 24, 0
VECTOR 25, 0
VECTOR 26, 0
VECTOR 27, 0
VECTOR 28, 0
VECTOR 29, 1
VECTOR 30, 1
VECTOR 31, 0
VECTOR 32, 0
VECTOR 33, 0
VECTOR 34, 0
VECTOR 35, 0
VECTOR 36, 0
VECTOR 37, 0
VECTOR 38, 0
VECTOR 39, 0
VECTOR 40, 0
VECTOR 41, 0
VECTOR 42, 0
VECTOR 43, 0
VECTOR 44, 0
VECTOR 45, 0
VECTOR 46, 0
VECTOR 47, 0
VECTOR 48, 0
VECTOR 49, 0
VECTOR 50, 0
VECTOR 51, 0
VECTOR 52, 0
VECTOR 53, 0
VECTOR 54, 0
VECTOR 55, 0
VECTOR 56, 0
VECTOR 57, 0
VECTOR 58, 0
VECTOR 59, 0
VECTOR 60, 0
VECTOR 61, 0
VECTOR 62, 0
VECTOR 63, 0
VECTOR 64, 0
VECTOR 65, 0
VECTOR 66, 0
VECTOR 67, 0
VECTOR 68, 0
VECTOR 69, 0
VECTOR 70, 0
VECTOR 71, 0
VECTOR 72, 0
VECTOR 73, 0
VECTOR 74, 0
VECTOR 75, 0
VECTOR 76, 0
VECTOR 77, 0
VECTOR 78, 0
VECTOR 79, 0
VECTOR 80, 0
VECTOR 81, 0
VECTOR 82, 0
VECTOR 83, 0
VECTOR 84, 0
VECTOR 85, 0
VECTOR 86, 0
VECTOR 87, 0
VECTOR 88, 0
VECTOR 89, 0
VECTOR 90, 0
VECTOR 91, 0
VECTOR 92, 0
VECTOR 93, 0
VECTOR 94, 0
VECTOR 95, 0
VECTOR 96, 0
VECTOR 97, 0
VECTOR 98, 0
VECTOR 99, 0
VECTOR 100, 0
VECTOR 101, 0
VECTOR 102, 0
VECTOR 103, 0
VECTOR 104, 0
VECTOR 105, 0
VECTOR 106, 0
VECTOR 107, 0
VECTOR 108, 0
VECTOR 109, 0
VECTOR 110, 0
VECTOR 111, 0
VECTOR 112, 0
VECTOR 113, 0
VECTOR 114, 0
VECTOR 115, 0
VECTOR 116, 0
VECTOR 117, 0
VECTOR 118, 0
VECTOR 119, 0
VECTOR 120, 0
VECTOR 121, 0
VECTOR 122, 0
VECTOR 123, 0
VECTOR 124, 0
VECTOR 125, 0
VECTOR 126, 0
VECTOR 127, 0
VECTOR 128, 0
VECTOR 129, 0
VECTOR 130, 0
VECTOR 131, 0
VECTOR 132, 0
VECTOR 133, 0
VECTOR 134, 0
VECTOR 135, 0
VECTOR 136, 0
VECTOR 137, 0
VECTOR 138, 0
VECTOR 139, 0
VECTOR 140, 0
VECTOR 141, 0
VECTOR 142, 0
VECTOR 143, 0
VECTOR 144, 0
VECTOR 145, 0
VECTOR 146, 0
VECTOR 147, 0
VECTOR 148, 0
VECTOR 149, 0
VECTOR 150, 0
VECTOR 151, 0
VECTOR 152, 0
VECTOR 153, 0
VECTOR 154, 0
VECTOR 155, 0
VECTOR 156, 0
VECTOR 157, 0
VECTOR 158, 0
VECTOR 159, 0
VECTOR 160, 0
VECTOR 161, 0
VECTOR 162, 0
VECTOR 163, 0
VECTOR 164, 0
VECTOR 165, 0
VECTOR 166, 0
VECTOR 167, 0
VECTOR 168, 0
VECTOR 169, 0
VECTOR 170, 0
VECTOR 171, 0
VECTOR 172, 0
VECTOR 173, 0
VECTOR 174, 0
VECTOR 175, 0
VECTOR 176, 0
VECTOR 177, 0
VECTOR 178, 0
VECTOR 179, 0
VECTOR 180, 0
VECTOR 181, 0
VECTOR 182, 0
VECTOR 183, 0
VECTOR 184, 0
VECTOR 185, 0
VECTOR 186, 0
VECTOR 187, 0
VECTOR 188, 0
VECTOR 189, 0
VECTOR 190, 0
VECTOR 191, 0
VECTOR 192, 0
VECTOR 193, 0
VECTOR 194, 0
VECTOR 195, 0
VECTOR 196, 0
VECTOR 197, 0
VECTOR 198, 0
VECTOR 199, 0
VECTOR 200, 0
VECTOR 201, 0
VECTOR 202, 0
VECTOR 203, 0
VECTOR 204, 0
VECTOR 205, 0
VECTOR 206, 0
VECTOR 207, 0
VECTOR 208, 0
VECTOR 209, 0
VECTOR 210, 0
VECTOR 211, 0
VECTOR 212, 0
VECTOR 213, 0
VECTOR 214, 0
VECTOR 215, 0
VECTOR 216, 0
VECTOR 217, 0
VECTOR 218, 0
VECTOR 219, 0
VECTOR 220, 0
VECTOR 221, 0
VECTOR 222, 0
VECTOR 223, 0
VECTOR 224, 0
VECTOR 225, 0
VECTOR 226, 0
VECTOR 227, 0
VECTOR 228, 0
VECTOR 229, 0
VECTOR 230, 0
VECTOR 231, 0
VECTOR 232, 0
VECTOR 233, 0
VECTOR 234, 0
VECTOR 235, 0
VECTOR 236, 0
VECTOR 237, 0
VECTOR 238, 0
VECTOR 239, 0
VECTOR 240, 0
VECTOR 241, 0
VECTOR 242, 0
VECTOR 243, 0
VECTOR 244, 0
VECTOR 245, 0
VECTOR 246, 0
VECTOR 247, 0
VECTOR 248, 0
VECTOR 249, 0
VECTOR 250, 0
VECTOR 251, 0
VECTOR 252, 0
VECTOR 253, 0
VECTOR 254, 0
VECTOR 255, 0

.section .rdata,"dr"
.p2align 3
jihoo_vector_table:
    .quad jihoo_vector_0
    .quad jihoo_vector_1
    .quad jihoo_vector_2
    .quad jihoo_vector_3
    .quad jihoo_vector_4
    .quad jihoo_vector_5
    .quad jihoo_vector_6
    .quad jihoo_vector_7
    .quad jihoo_vector_8
    .quad jihoo_vector_9
    .quad jihoo_vector_10
    .quad jihoo_vector_11
    .quad jihoo_vector_12
    .quad jihoo_vector_13
    .quad jihoo_vector_14
    .quad jihoo_vector_15
    .quad jihoo_vector_16
    .quad jihoo_vector_17
    .quad jihoo_vector_18
    .quad jihoo_vector_19
    .quad jihoo_vector_20
    .quad jihoo_vector_21
    .quad jihoo_vector_22
    .quad jihoo_vector_23
    .quad jihoo_vector_24
    .quad jihoo_vector_25
    .quad jihoo_vector_26
    .quad jihoo_vector_27
    .quad jihoo_vector_28
    .quad jihoo_vector_29
    .quad jihoo_vector_30
    .quad jihoo_vector_31
    .quad jihoo_vector_32
    .quad jihoo_vector_33
    .quad jihoo_vector_34
    .quad jihoo_vector_35
    .quad jihoo_vector_36
    .quad jihoo_vector_37
    .quad jihoo_vector_38
    .quad jihoo_vector_39
    .quad jihoo_vector_40
    .quad jihoo_vector_41
    .quad jihoo_vector_42
    .quad jihoo_vector_43
    .quad jihoo_vector_44
    .quad jihoo_vector_45
    .quad jihoo_vector_46
    .quad jihoo_vector_47
    .quad jihoo_vector_48
    .quad jihoo_vector_49
    .quad jihoo_vector_50
    .quad jihoo_vector_51
    .quad jihoo_vector_52
    .quad jihoo_vector_53
    .quad jihoo_vector_54
    .quad jihoo_vector_55
    .quad jihoo_vector_56
    .quad jihoo_vector_57
    .quad jihoo_vector_58
    .quad jihoo_vector_59
    .quad jihoo_vector_60
    .quad jihoo_vector_61
    .quad jihoo_vector_62
    .quad jihoo_vector_63
    .quad jihoo_vector_64
    .quad jihoo_vector_65
    .quad jihoo_vector_66
    .quad jihoo_vector_67
    .quad jihoo_vector_68
    .quad jihoo_vector_69
    .quad jihoo_vector_70
    .quad jihoo_vector_71
    .quad jihoo_vector_72
    .quad jihoo_vector_73
    .quad jihoo_vector_74
    .quad jihoo_vector_75
    .quad jihoo_vector_76
    .quad jihoo_vector_77
    .quad jihoo_vector_78
    .quad jihoo_vector_79
    .quad jihoo_vector_80
    .quad jihoo_vector_81
    .quad jihoo_vector_82
    .quad jihoo_vector_83
    .quad jihoo_vector_84
    .quad jihoo_vector_85
    .quad jihoo_vector_86
    .quad jihoo_vector_87
    .quad jihoo_vector_88
    .quad jihoo_vector_89
    .quad jihoo_vector_90
    .quad jihoo_vector_91
    .quad jihoo_vector_92
    .quad jihoo_vector_93
    .quad jihoo_vector_94
    .quad jihoo_vector_95
    .quad jihoo_vector_96
    .quad jihoo_vector_97
    .quad jihoo_vector_98
    .quad jihoo_vector_99
    .quad jihoo_vector_100
    .quad jihoo_vector_101
    .quad jihoo_vector_102
    .quad jihoo_vector_103
    .quad jihoo_vector_104
    .quad jihoo_vector_105
    .quad jihoo_vector_106
    .quad jihoo_vector_107
    .quad jihoo_vector_108
    .quad jihoo_vector_109
    .quad jihoo_vector_110
    .quad jihoo_vector_111
    .quad jihoo_vector_112
    .quad jihoo_vector_113
    .quad jihoo_vector_114
    .quad jihoo_vector_115
    .quad jihoo_vector_116
    .quad jihoo_vector_117
    .quad jihoo_vector_118
    .quad jihoo_vector_119
    .quad jihoo_vector_120
    .quad jihoo_vector_121
    .quad jihoo_vector_122
    .quad jihoo_vector_123
    .quad jihoo_vector_124
    .quad jihoo_vector_125
    .quad jihoo_vector_126
    .quad jihoo_vector_127
    .quad jihoo_vector_128
    .quad jihoo_vector_129
    .quad jihoo_vector_130
    .quad jihoo_vector_131
    .quad jihoo_vector_132
    .quad jihoo_vector_133
    .quad jihoo_vector_134
    .quad jihoo_vector_135
    .quad jihoo_vector_136
    .quad jihoo_vector_137
    .quad jihoo_vector_138
    .quad jihoo_vector_139
    .quad jihoo_vector_140
    .quad jihoo_vector_141
    .quad jihoo_vector_142
    .quad jihoo_vector_143
    .quad jihoo_vector_144
    .quad jihoo_vector_145
    .quad jihoo_vector_146
    .quad jihoo_vector_147
    .quad jihoo_vector_148
    .quad jihoo_vector_149
    .quad jihoo_vector_150
    .quad jihoo_vector_151
    .quad jihoo_vector_152
    .quad jihoo_vector_153
    .quad jihoo_vector_154
    .quad jihoo_vector_155
    .quad jihoo_vector_156
    .quad jihoo_vector_157
    .quad jihoo_vector_158
    .quad jihoo_vector_159
    .quad jihoo_vector_160
    .quad jihoo_vector_161
    .quad jihoo_vector_162
    .quad jihoo_vector_163
    .quad jihoo_vector_164
    .quad jihoo_vector_165
    .quad jihoo_vector_166
    .quad jihoo_vector_167
    .quad jihoo_vector_168
    .quad jihoo_vector_169
    .quad jihoo_vector_170
    .quad jihoo_vector_171
    .quad jihoo_vector_172
    .quad jihoo_vector_173
    .quad jihoo_vector_174
    .quad jihoo_vector_175
    .quad jihoo_vector_176
    .quad jihoo_vector_177
    .quad jihoo_vector_178
    .quad jihoo_vector_179
    .quad jihoo_vector_180
    .quad jihoo_vector_181
    .quad jihoo_vector_182
    .quad jihoo_vector_183
    .quad jihoo_vector_184
    .quad jihoo_vector_185
    .quad jihoo_vector_186
    .quad jihoo_vector_187
    .quad jihoo_vector_188
    .quad jihoo_vector_189
    .quad jihoo_vector_190
    .quad jihoo_vector_191
    .quad jihoo_vector_192
    .quad jihoo_vector_193
    .quad jihoo_vector_194
    .quad jihoo_vector_195
    .quad jihoo_vector_196
    .quad jihoo_vector_197
    .quad jihoo_vector_198
    .quad jihoo_vector_199
    .quad jihoo_vector_200
    .quad jihoo_vector_201
    .quad jihoo_vector_202
    .quad jihoo_vector_203
    .quad jihoo_vector_204
    .quad jihoo_vector_205
    .quad jihoo_vector_206
    .quad jihoo_vector_207
    .quad jihoo_vector_208
    .quad jihoo_vector_209
    .quad jihoo_vector_210
    .quad jihoo_vector_211
    .quad jihoo_vector_212
    .quad jihoo_vector_213
    .quad jihoo_vector_214
    .quad jihoo_vector_215
    .quad jihoo_vector_216
    .quad jihoo_vector_217
    .quad jihoo_vector_218
    .quad jihoo_vector_219
    .quad jihoo_vector_220
    .quad jihoo_vector_221
    .quad jihoo_vector_222
    .quad jihoo_vector_223
    .quad jihoo_vector_224
    .quad jihoo_vector_225
    .quad jihoo_vector_226
    .quad jihoo_vector_227
    .quad jihoo_vector_228
    .quad jihoo_vector_229
    .quad jihoo_vector_230
    .quad jihoo_vector_231
    .quad jihoo_vector_232
    .quad jihoo_vector_233
    .quad jihoo_vector_234
    .quad jihoo_vector_235
    .quad jihoo_vector_236
    .quad jihoo_vector_237
    .quad jihoo_vector_238
    .quad jihoo_vector_239
    .quad jihoo_vector_240
    .quad jihoo_vector_241
    .quad jihoo_vector_242
    .quad jihoo_vector_243
    .quad jihoo_vector_244
    .quad jihoo_vector_245
    .quad jihoo_vector_246
    .quad jihoo_vector_247
    .quad jihoo_vector_248
    .quad jihoo_vector_249
    .quad jihoo_vector_250
    .quad jihoo_vector_251
    .quad jihoo_vector_252
    .quad jihoo_vector_253
    .quad jihoo_vector_254
    .quad jihoo_vector_255
