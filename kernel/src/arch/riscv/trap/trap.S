
    .section .text.trap
    .globl trap_entry
    .globl trap_resume
    .align 2
trap_entry:

    # Swap user TLS with the current Task pointer. ThreadInfo is at offset 0. 
    csrrw tp, sscratch, tp

    # On kernel traps, sscratch is 0, so we just load it back in.
    bnez tp, save_user_stack
    csrr tp, sscratch
    j save_kernel_stack

# On user threads, user sp needs to be stored and kernel sp needs to be loaded
save_user_stack:
    # Save the sp to `user_sp` under `ThreadInfo` ..
    sd sp, 0*8(tp)
    # .. and load the `kernel_sp`.
    ld sp, 1*8(tp)

    addi sp, sp, -{TRAPFRAME_SIZE}

    # Store `ra` in trap frame. This is done before the rest of the saves because we
    # use is as a scratch register here.
    sd ra,  {TF_RA}(sp)
    # Save the `user_sp` to the trap frame.
    ld ra, 0*8(tp)
    sd ra,  {TF_SP}(sp)
    j save_trapframe

save_kernel_stack:
    addi sp, sp, -{TRAPFRAME_SIZE}
    # Again temporarily use `ra` to save the original `sp` to the trap frame.
    sd ra,  {TF_RA}(sp)
    addi ra, sp, {TRAPFRAME_SIZE}
    sd ra,  {TF_SP}(sp)

save_trapframe:
    # Read the original `tp` (since we did `csrrw` before) and save it to the
    # trap frame.
    csrr ra, sscratch
    sd ra,  {TF_TP}(sp)
    ld ra,  {TF_RA}(sp)
    csrw sscratch, zero
    sd gp,  {TF_GP}(sp)
    sd t0,  {TF_T0}(sp)
    sd t1,  {TF_T1}(sp)
    sd t2,  {TF_T2}(sp)
    sd s0,  {TF_S0}(sp)
    sd s1,  {TF_S1}(sp)
    sd a0,  {TF_A0}(sp)
    sd a1,  {TF_A1}(sp)
    sd a2,  {TF_A2}(sp)
    sd a3,  {TF_A3}(sp)
    sd a4,  {TF_A4}(sp)
    sd a5,  {TF_A5}(sp)
    sd a6,  {TF_A6}(sp)
    sd a7,  {TF_A7}(sp)
    sd s2,  {TF_S2}(sp)
    sd s3,  {TF_S3}(sp)
    sd s4,  {TF_S4}(sp)
    sd s5,  {TF_S5}(sp)
    sd s6,  {TF_S6}(sp)
    sd s7,  {TF_S7}(sp)
    sd s8,  {TF_S8}(sp)
    sd s9,  {TF_S9}(sp)
    sd s10, {TF_S10}(sp)
    sd s11, {TF_S11}(sp)
    sd t3,  {TF_T3}(sp)
    sd t4,  {TF_T4}(sp)
    sd t5,  {TF_T5}(sp)
    sd t6,  {TF_T6}(sp)

    csrr t0, sepc
    sd t0, {TF_SEPC}(sp)

    csrr t0, scause
    sd t0, {TF_SCAUSE}(sp)

    csrr t0, sstatus
    sd t0, {TF_SSTATUS}(sp)
   
    # `trap_handler` gets the trap frame pointer as a param
    mv a0, sp
    call trap_handler

# Note that this is also a global symbol and all the userspace applications
# start from here. But the task manager ensures their stack is set in a way
# it works as if they are resuming after trapping.
trap_resume:
    ld t0, {TF_SEPC}(sp)
    csrw sepc, t0

    ld t0, {TF_SSTATUS}(sp)
    csrw sstatus, t0

    ld ra,  {TF_RA}(sp)
    ld gp,  {TF_GP}(sp)
    ld t2,  {TF_T2}(sp)
    ld s0,  {TF_S0}(sp)
    ld s1,  {TF_S1}(sp)
    ld a0,  {TF_A0}(sp)
    ld a1,  {TF_A1}(sp)
    ld a2,  {TF_A2}(sp)
    ld a3,  {TF_A3}(sp)
    ld a4,  {TF_A4}(sp)
    ld a5,  {TF_A5}(sp)
    ld a6,  {TF_A6}(sp)
    ld a7,  {TF_A7}(sp)
    ld s2,  {TF_S2}(sp)
    ld s3,  {TF_S3}(sp)
    ld s4,  {TF_S4}(sp)
    ld s5,  {TF_S5}(sp)
    ld s6,  {TF_S6}(sp)
    ld s7,  {TF_S7}(sp)
    ld s8,  {TF_S8}(sp)
    ld s9,  {TF_S9}(sp)
    ld s10, {TF_S10}(sp)
    ld s11, {TF_S11}(sp)
    ld t3,  {TF_T3}(sp)
    ld t4,  {TF_T4}(sp)
    ld t5,  {TF_T5}(sp)
    ld t6,  {TF_T6}(sp)

    # If saved sstatus.SPP is set, the trap came from supervisor mode.
    # Kernel traps use sscratch = 0; user traps use sscratch = task ptr.
    ld t0, {TF_SSTATUS}(sp)
    li t1, {SSTATUS_SPP_MASK}
    and t0, t0, t1
    bnez t0, ret_kernel

ret_userspace:
    csrw sscratch, tp
    ld t0, {TF_T0}(sp)
    ld t1, {TF_T1}(sp)
    ld tp, {TF_TP}(sp)
    ld sp, {TF_SP}(sp)
    sret

ret_kernel:
    csrw sscratch, zero
    ld t0, {TF_T0}(sp)
    ld t1, {TF_T1}(sp)
    ld sp, {TF_SP}(sp)
    sret
