/*
 * MPU-based peripheral-read capture (EXPERIMENTAL). See backstep_mmio.h
 * and docs/adr/0003-peripheral-read-capture.md.
 *
 * Target-only code: compiled into firmware for ARMv7-M cores with an MPU.
 *
 * SPDX-License-Identifier: MIT
 */
#include "backstep_mmio.h"
#include "thumb_ldst.h"

#define REG32(addr) (*(volatile uint32_t *)(addr))

#define SCB_SHPR1 REG32(0xE000ED18u)
#define SCB_SHCSR REG32(0xE000ED24u)
#define SCB_CFSR REG32(0xE000ED28u)
#define SCB_HFSR REG32(0xE000ED2Cu)
#define SCB_MMFAR REG32(0xE000ED34u)
#define MPU_TYPE REG32(0xE000ED90u)
#define MPU_CTRL REG32(0xE000ED94u)
#define MPU_RNR REG32(0xE000ED98u)
#define MPU_RBAR REG32(0xE000ED9Cu)
#define MPU_RASR REG32(0xE000EDA0u)

#define SHCSR_MEMFAULTENA (1u << 16)
#define MMFSR_DACCVIOL (1u << 1)
#define MMFSR_MMARVALID (1u << 7)
#define MMFSR_ALL 0xFFu
#define HFSR_FORCED (1u << 30)
#define MPU_CTRL_ENABLE (1u << 0)
#define MPU_CTRL_PRIVDEFENA (1u << 2)

/* MPU region 0 attributes: XN, no access, shared device. The SIZE field
 * encodes 2^(SIZE+1) bytes; the default region is 512 MB (SIZE = 28). */
#define TRAP_RASR_ATTR ((1u << 28) /* XN */ | (0u << 24) /* AP: none */ | \
                        (1u << 18) /* S */ | (1u << 16) /* B */ |          \
                        1u /* ENABLE */)

/* Exception frame pushed by hardware, plus r4-r11 pushed by the stub. */
enum { F_R0, F_R1, F_R2, F_R3, F_R12, F_LR, F_PC, F_XPSR };

static struct bs_mmio_event *s_log;
static uint32_t s_capacity;
static uint32_t s_region_base = BS_MMIO_REGION_BASE;
static uint32_t s_region_size = BS_MMIO_REGION_SIZE;
static volatile struct bs_mmio_trap_stats s_stats;

static inline void barrier(void)
{
    __asm__ volatile("dsb\n\tisb" ::: "memory");
}

static uint32_t *reg_slot(uint32_t *frame, uint32_t *callee, unsigned r)
{
    switch (r) {
    case 0: return &frame[F_R0];
    case 1: return &frame[F_R1];
    case 2: return &frame[F_R2];
    case 3: return &frame[F_R3];
    case 12: return &frame[F_R12];
    case 14: return &frame[F_LR];
    default:
        return (r >= 4u && r <= 11u) ? &callee[r - 4u] : (uint32_t *)0;
    }
}

/* Default: halt visibly instead of corrupting the run. Firmware (and the
 * test suite) may override this weak symbol to report the problem. */
__attribute__((weak)) void bs_mmio_trap_fatal(uint32_t pc, uint32_t reason)
{
    (void)pc;
    (void)reason;
    __asm__ volatile("bkpt 0x42");
    for (;;) {
    }
}

static void stop_core(uint32_t pc, uint32_t reason)
{
    s_stats.unsupported++;
    bs_mmio_trap_fatal(pc, reason);
    for (;;) {
        /* bs_mmio_trap_fatal() must not return; never resume if it does. */
    }
}

static int in_trap_region(uint32_t addr)
{
    return (addr - s_region_base) < s_region_size;
}

/*
 * Emulate the faulting peripheral access. Returns 1 if handled, 0 if the
 * fault was not ours (caller then treats it as a real fault).
 */
static int handle(uint32_t *frame, uint32_t *callee)
{
    uint32_t mmfsr = SCB_CFSR & MMFSR_ALL;
    uint32_t fault_addr = SCB_MMFAR;
    uint32_t pc = frame[F_PC];
    const uint16_t *code = (const uint16_t *)pc;
    struct bs_ldst insn;
    uint32_t *rt, *rn, *rm;
    uint32_t addr, new_rn, value = 0u;
    uint32_t ctrl;

    if ((mmfsr & (MMFSR_DACCVIOL | MMFSR_MMARVALID)) !=
            (MMFSR_DACCVIOL | MMFSR_MMARVALID) ||
        !in_trap_region(fault_addr)) {
        return 0;
    }

    if (bs_decode_ldst(code[0], bs_thumb_is_32bit(code[0]) ? code[1] : 0u,
                       &insn) != BS_LDST_OK) {
        stop_core(pc, BS_MMIO_FATAL_UNSUPPORTED_INSN);
    }
    rt = reg_slot(frame, callee, insn.rt);
    rn = reg_slot(frame, callee, insn.rn);
    rm = insn.offset_is_register ? reg_slot(frame, callee, insn.rm) : rn;
    if (rt == 0 || rn == 0 || rm == 0) {
        stop_core(pc, BS_MMIO_FATAL_UNSUPPORTED_INSN);
    }
    bs_ldst_address(&insn, *rn, *rm, &addr, &new_rn);
    if (addr != fault_addr) {
        /* Decoder and hardware disagree: never guess. */
        stop_core(pc, BS_MMIO_FATAL_ADDRESS_MISMATCH);
    }

    /* Perform the access with the MPU off, exactly once, at its size. */
    ctrl = MPU_CTRL;
    MPU_CTRL = 0u;
    barrier();
    if (insn.is_load) {
        switch (insn.size) {
        case 1: value = *(volatile uint8_t *)addr; break;
        case 2: value = *(volatile uint16_t *)addr; break;
        default: value = *(volatile uint32_t *)addr; break;
        }
    } else {
        switch (insn.size) {
        case 1: *(volatile uint8_t *)addr = (uint8_t)*rt; break;
        case 2: *(volatile uint16_t *)addr = (uint16_t)*rt; break;
        default: *(volatile uint32_t *)addr = *rt; break;
        }
    }
    MPU_CTRL = ctrl;
    barrier();

    if (insn.is_load) {
        uint32_t n = s_stats.loads;
        if (n < s_capacity) {
            s_log[n].address = addr;
            s_log[n].value = value;
            s_log[n].pc = pc;
            s_log[n].size = insn.size;
        } else {
            s_stats.dropped++;
        }
        s_stats.loads = n + 1u;
        if (insn.sign_extend) {
            value = insn.size == 1u ? (uint32_t)(int32_t)(int8_t)value
                                    : (uint32_t)(int32_t)(int16_t)value;
        }
    } else {
        s_stats.stores++;
    }

    /* Writeback happens before the load result, matching the hardware
     * when Rn != Rt (Rn == Rt with writeback is rejected by the decoder). */
    *rn = new_rn;
    if (insn.is_load) {
        *rt = value;
    }
    frame[F_PC] = pc + insn.length;
    frame[F_XPSR] = bs_xpsr_it_advance(frame[F_XPSR]);
    SCB_CFSR = mmfsr; /* write-one-to-clear */
    return 1;
}

void bs_mmio_trap_memmanage(uint32_t *frame, uint32_t *callee);
void bs_mmio_trap_hardfault(uint32_t *frame, uint32_t *callee);

void bs_mmio_trap_memmanage(uint32_t *frame, uint32_t *callee)
{
    if (!handle(frame, callee)) {
        stop_core(frame[F_PC], BS_MMIO_FATAL_REAL_FAULT);
    }
}

void bs_mmio_trap_hardfault(uint32_t *frame, uint32_t *callee)
{
    /* Escalated MemManage: the access happened at a priority that could
     * not take MemManage (e.g. an interrupt at priority 0). */
    if ((SCB_HFSR & HFSR_FORCED) && handle(frame, callee)) {
        SCB_HFSR = HFSR_FORCED;
        return;
    }
    stop_core(frame[F_PC], BS_MMIO_FATAL_REAL_FAULT);
}

/*
 * Naked entry stubs: pick the stack that holds the exception frame, save
 * r4-r11 so the C handler can read and modify them, and restore them on
 * the way out.
 */
#define BS_FAULT_STUB(name, c_handler)                                       \
    __attribute__((naked)) void name(void)                                   \
    {                                                                        \
        __asm__ volatile("tst lr, #4\n\t"                                   \
                         "ite eq\n\t"                                        \
                         "mrseq r0, msp\n\t"                                 \
                         "mrsne r0, psp\n\t"                                 \
                         "push {r4-r11, lr}\n\t"                             \
                         "sub sp, #4\n\t" /* keep 8-byte stack alignment */  \
                         "add r1, sp, #4\n\t"                                \
                         "bl " #c_handler "\n\t"                             \
                         "add sp, #4\n\t"                                    \
                         "pop {r4-r11, lr}\n\t"                              \
                         "bx lr\n\t");                                       \
    }

BS_FAULT_STUB(bs_memmanage_handler, bs_mmio_trap_memmanage)
BS_FAULT_STUB(bs_hardfault_handler, bs_mmio_trap_hardfault)

int bs_mmio_trap_enable_region(struct bs_mmio_event *log, uint32_t capacity,
                               uint32_t base, uint32_t size_log2)
{
    uint32_t size;

    if (((MPU_TYPE >> 8) & 0xFFu) == 0u) {
        return -1; /* no data regions: no MPU */
    }
    if (size_log2 < 5u || size_log2 > 31u) {
        return -1;
    }
    size = 1u << size_log2;
    if ((base & (size - 1u)) != 0u) {
        return -1; /* MPU regions must be aligned to their size */
    }
    s_log = log;
    s_capacity = capacity;
    s_region_base = base;
    s_region_size = size;
    s_stats.loads = 0u;
    s_stats.stores = 0u;
    s_stats.dropped = 0u;
    s_stats.unsupported = 0u;

    /* MemManage at the highest configurable priority so it preempts all
     * interrupts that run at a lower (numerically higher) priority. */
    SCB_SHPR1 &= ~0xFFu;
    SCB_SHCSR |= SHCSR_MEMFAULTENA;

    MPU_CTRL = 0u;
    barrier();
    MPU_RNR = 0u;
    MPU_RBAR = base | (1u << 4) /* VALID */ | 0u /* region 0 */;
    MPU_RASR = TRAP_RASR_ATTR | ((size_log2 - 1u) << 1);
    MPU_CTRL = MPU_CTRL_ENABLE | MPU_CTRL_PRIVDEFENA;
    barrier();
    return 0;
}

int bs_mmio_trap_enable(struct bs_mmio_event *log, uint32_t capacity)
{
    return bs_mmio_trap_enable_region(log, capacity, BS_MMIO_REGION_BASE, 29u);
}

void bs_mmio_trap_disable(void)
{
    MPU_CTRL = 0u;
    barrier();
    MPU_RNR = 0u;
    MPU_RASR = 0u;
    barrier();
}

const volatile struct bs_mmio_trap_stats *bs_mmio_trap_stats(void)
{
    return &s_stats;
}
