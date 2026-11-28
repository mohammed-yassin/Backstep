/*
 * Recording (EXPERIMENTAL, milestone M3). See backstep_record.h and
 * docs/adr/0008-recording-format.md.
 *
 * SPDX-License-Identifier: MIT
 */
#include "bs_no_fpu.h"
#include "backstep_record.h"

#include "backstep.h"

enum { REC_NONE = 0, REC_RECORDING = 1, REC_DONE = 2 };

/* All of the recorder's state is in .backstep, outside the program's RAM
 * as snapshotted and checksummed, so recording does not change it. */
static struct {
    struct bs_record_config cfg;
    struct bs_cpu_state start_cpu;
    struct bs_snapshot_header snap;
    struct bs_record_end_state end;
    struct bs_fp_state fp[2]; /* at the start and at the end */
    uint32_t snap_bytes; /* RAM bytes copied into cfg.ram_buffer */
    uint32_t events;
    uint32_t dropped;
    uint32_t irq_events;
    uint32_t irq_dropped;
    uint32_t state;
    uint32_t ok;
} g_rec BS_STATE;

static uint32_t span(const void *start, const void *end)
{
    return (uint32_t)((uintptr_t)end - (uintptr_t)start);
}

/* Runs inside bs_snapshot_take(), on stack below the caller's SP: copies
 * the program's RAM, touching only .backstep. No peripheral access here:
 * capture is already on. */
static void record_save(const struct bs_cpu_state *cpu)
{
    struct bs_region regions[2];
    uint32_t i, r, n = 0u;

    regions[0].address = (uint32_t)(uintptr_t)g_rec.cfg.ram_start;
    regions[0].length = span(g_rec.cfg.ram_start, g_rec.cfg.ram_end);
    /* The whole stack area: replay then starts with the same dead stack
     * too. The part save() is using is stored as the zeros that
     * bs_snapshot_take() writes there when save() returns. */
    regions[1].address = (uint32_t)(uintptr_t)g_rec.cfg.stack_bottom;
    regions[1].length = span(g_rec.cfg.stack_bottom, g_rec.cfg.stack_top);
    bs_snapshot_header_build(&g_rec.snap, cpu, regions, 2u);
    g_rec.fp[0] = bs_snapshot_fp;
    for (r = 0; r < 2u; ++r) {
        const uint8_t *src = (const uint8_t *)(uintptr_t)regions[r].address;
        if (regions[r].length > g_rec.cfg.ram_buffer_capacity - n) {
            g_rec.ok = 0u;
            return;
        }
        for (i = 0; i < regions[r].length; ++i) {
            uint32_t a = regions[r].address + i;
            int scratch = a >= cpu->sp - BS_SNAPSHOT_SAVE_BYTES && a < cpu->sp;
            g_rec.cfg.ram_buffer[n++] = scratch ? 0u : src[i];
        }
    }
    /* The snapshot's CRC must cover the bytes as stored. */
    g_rec.snap.regions_crc32 = bs_crc32_update(0u, g_rec.cfg.ram_buffer, n);
    g_rec.snap_bytes = n;
    if (bs_mmio_trap_stats()->loads != 0u) {
        g_rec.ok = 0u; /* nothing may be captured before the snapshot */
    }
    g_rec.state = REC_RECORDING;
}

/* Runs inside the final bs_snapshot_take(), below the caller's SP, so the
 * stack above it is checksummed exactly as it was at the end point. */
static void record_finish(const struct bs_cpu_state *cpu)
{
    const volatile struct bs_mmio_trap_stats *st;
    struct bs_record_end_state *e = &g_rec.end;

    bs_mmio_trap_disable();
    bs_irq_record_stop();
    g_rec.fp[1] = bs_snapshot_fp;
    st = bs_mmio_trap_stats();
    g_rec.events = st->loads - st->dropped;
    g_rec.dropped = st->dropped;
    {
        uint32_t n = bs_irq_record_count();
        g_rec.irq_events = n < g_rec.cfg.irq_capacity ? n : g_rec.cfg.irq_capacity;
        g_rec.irq_dropped = n - g_rec.irq_events;
    }
    e->region_count = 2u;
    e->regions[0].address = (uint32_t)(uintptr_t)g_rec.cfg.ram_start;
    e->regions[0].length = span(g_rec.cfg.ram_start, g_rec.cfg.ram_end);
    e->regions[1].address = cpu->sp;
    e->regions[1].length = (uint32_t)(uintptr_t)g_rec.cfg.stack_top - cpu->sp;
    e->ram_crc32 = bs_crc32_update(0u, (const void *)(uintptr_t)e->regions[0].address,
                                   e->regions[0].length);
    e->ram_crc32 = bs_crc32_update(e->ram_crc32,
                                   (const void *)(uintptr_t)e->regions[1].address,
                                   e->regions[1].length);
    if (g_rec.state == REC_RECORDING) {
        g_rec.state = REC_DONE;
    }
}

static uint32_t primask(void)
{
    uint32_t v;
    __asm__ volatile("mrs %0, primask" : "=r"(v));
    return v;
}

int bs_record_begin(const struct bs_record_config *cfg)
{
    uint32_t masked = primask();
    int r;

    /* No interrupt may arrive between starting the capture and the
     * snapshot: it would be recorded before the recording starts. */
    __asm__ volatile("cpsid i" ::: "memory");
    g_rec.cfg = *cfg;
    g_rec.ok = 1u;
    g_rec.state = REC_NONE;
    if (bs_mmio_trap_enable(cfg->log, cfg->log_capacity) != 0) {
        if (masked == 0u) {
            __asm__ volatile("cpsie i" ::: "memory");
        }
        return -1;
    }
    /* Always started, also without a log: an interrupt entering through a
     * stub then marks the recording as having dropped events. */
    bs_irq_record_start(cfg->irq_log, cfg->irq_capacity, &bs_mmio_trap_stats()->loads,
                        cfg->stack_top);
    /* The recording starts where this returns (with 0), still masked;
     * the caller's PRIMASK comes back below, in the recorded stretch. */
    r = bs_snapshot_take(&g_rec.start_cpu, record_save);
    if (masked == 0u) {
        __asm__ volatile("cpsie i" ::: "memory");
    }
    return r;
}

void bs_record_end(void)
{
    /* Interrupts after this point would be recorded but never replayed. */
    __asm__ volatile("cpsid i" ::: "memory");
    /* Replay stops at the entry of this bs_snapshot_take() call. The CPU
     * state is in g_rec.end.cpu, which is already in .backstep. */
    (void)bs_snapshot_take(&g_rec.end.cpu, record_finish);
}

struct writer {
    void (*put)(const void *data, uint32_t len, void *ctx);
    void *ctx;
    uint32_t file_crc;
    uint32_t bytes;
};

static void emit(struct writer *w, const void *data, uint32_t len)
{
    w->file_crc = bs_crc32_update(w->file_crc, data, len);
    w->bytes += len;
    w->put(data, len, w->ctx);
}

/* One chunk with a payload of up to two parts. */
static void chunk(struct writer *w, uint32_t type, const void *a, uint32_t a_len,
                  const void *b, uint32_t b_len)
{
    uint32_t head[2];
    uint32_t crc;
    head[0] = type;
    head[1] = a_len + b_len;
    emit(w, head, sizeof head);
    emit(w, a, a_len);
    crc = bs_crc32_update(0u, a, a_len);
    if (b_len != 0u) {
        emit(w, b, b_len);
        crc = bs_crc32_update(crc, b, b_len);
    }
    emit(w, &crc, sizeof crc);
}

uint32_t bs_record_write(void (*put)(const void *data, uint32_t len, void *ctx), void *ctx)
{
    struct writer w;
    uint32_t header[4], target[4], mmio[2];
    uint32_t image_len = span(g_rec.cfg.image_start, g_rec.cfg.image_end);

    if (g_rec.state != REC_DONE || g_rec.ok == 0u || g_rec.dropped != 0u ||
        g_rec.irq_dropped != 0u) {
        return 0u;
    }
    w.put = put;
    w.ctx = ctx;
    w.file_crc = 0u;
    w.bytes = 0u;
    header[0] = BS_REC_MAGIC;
    header[1] = BS_REC_VERSION;
    header[2] = sizeof header;
    header[3] = 6u; /* chunks */
    emit(&w, header, sizeof header);
    target[0] = g_rec.cfg.target_id;
    target[1] = bs_crc32(g_rec.cfg.image_start, image_len);
    target[2] = (uint32_t)(uintptr_t)g_rec.cfg.image_start;
    target[3] = image_len;
    chunk(&w, BS_REC_CHUNK_TARGET, target, sizeof target, 0, 0u);
    chunk(&w, BS_REC_CHUNK_SNAPSHOT, &g_rec.snap, sizeof g_rec.snap, g_rec.cfg.ram_buffer,
          g_rec.snap_bytes);
    mmio[0] = g_rec.events;
    mmio[1] = g_rec.dropped;
    chunk(&w, BS_REC_CHUNK_MMIO, mmio, sizeof mmio, g_rec.cfg.log,
          g_rec.events * (uint32_t)sizeof(struct bs_mmio_event));
    {
        uint32_t irqs[3];
        irqs[0] = g_rec.irq_events;
        irqs[1] = g_rec.irq_dropped;
        irqs[2] = (uint32_t)(uintptr_t)g_rec.cfg.stack_top;
        chunk(&w, BS_REC_CHUNK_IRQS, irqs, sizeof irqs, g_rec.cfg.irq_log,
              g_rec.irq_events * (uint32_t)sizeof(struct bs_irq_event));
    }
    chunk(&w, BS_REC_CHUNK_FPU, g_rec.fp, sizeof g_rec.fp, 0, 0u);
    chunk(&w, BS_REC_CHUNK_END, &g_rec.end, sizeof g_rec.end, 0, 0u);
    {
        uint32_t crc = w.file_crc;
        emit(&w, &crc, sizeof crc);
    }
    return w.bytes;
}
