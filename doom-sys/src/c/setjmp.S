/* ARMv8-M (thumbv8m.main) setjmp/longjmp for the RP2350 doom port.
 * jmp_buf layout (int[28]):
 *   [0..7]  r4-r11, [8] sp, [9] lr, [10..25] s16-s31 (callee-saved VFP), rest unused
 */
    .syntax unified
    .thumb

    .text
    .global setjmp
    .hidden setjmp
    .type setjmp, %function
setjmp:
    str     r4, [r0, #0]
    str     r5, [r0, #4]
    str     r6, [r0, #8]
    str     r7, [r0, #12]
    str     r8, [r0, #16]
    str     r9, [r0, #20]
    str     r10, [r0, #24]
    str     r11, [r0, #28]
    mov     r1, sp
    str     r1, [r0, #32]
    str     lr, [r0, #36]
    vstr    s16, [r0, #40]
    vstr    s17, [r0, #44]
    vstr    s18, [r0, #48]
    vstr    s19, [r0, #52]
    vstr    s20, [r0, #56]
    vstr    s21, [r0, #60]
    vstr    s22, [r0, #64]
    vstr    s23, [r0, #68]
    vstr    s24, [r0, #72]
    vstr    s25, [r0, #76]
    vstr    s26, [r0, #80]
    vstr    s27, [r0, #84]
    vstr    s28, [r0, #88]
    vstr    s29, [r0, #92]
    vstr    s30, [r0, #96]
    vstr    s31, [r0, #100]
    movs    r0, #0
    bx      lr

    .global longjmp
    .hidden longjmp
    .type longjmp, %function
longjmp:
    ldr     r4, [r0, #0]
    ldr     r5, [r0, #4]
    ldr     r6, [r0, #8]
    ldr     r7, [r0, #12]
    ldr     r8, [r0, #16]
    ldr     r9, [r0, #20]
    ldr     r10, [r0, #24]
    ldr     r11, [r0, #28]
    ldr     r2, [r0, #32]
    mov     sp, r2
    ldr     r3, [r0, #36]
    vldr    s16, [r0, #40]
    vldr    s17, [r0, #44]
    vldr    s18, [r0, #48]
    vldr    s19, [r0, #52]
    vldr    s20, [r0, #52]
    vldr    s21, [r0, #56]
    vldr    s22, [r0, #60]
    vldr    s23, [r0, #64]
    vldr    s24, [r0, #72]
    vldr    s25, [r0, #76]
    vldr    s26, [r0, #80]
    vldr    s27, [r0, #84]
    vldr    s28, [r0, #88]
    vldr    s29, [r0, #92]
    vldr    s30, [r0, #96]
    vldr    s31, [r0, #100]
    mov     r0, r1
    bx      r3
