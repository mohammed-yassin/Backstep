/*
 * Backstep recording (EXPERIMENTAL, milestone M3): record a stretch of a
 * program's execution so that `backstep replay` can re-run it exactly on
 * the PC.
 *
 *   bs_record_begin(&cfg);   // snapshot + start capturing peripheral reads
 *   ...code to record...     // polling code; interrupts are M4
 *   bs_record_end();         // capture the end state, stop capturing
 *   bs_record_write(put, ctx); // emit the recording file (after the end)
 *
 * A recording holds the starting-state snapshot (ADR 0005), every
 * peripheral load in order (ADR 0003), and the end state (registers and a
 * RAM checksum) that replay must reproduce. The file format is described in
 * docs/adr/0008-recording-format.md.
 *
 * Rules for the recorded code: interrupts must enter through BS_IRQ_STUB
 * (backstep_irq.h) so their positions are recorded; no MPU use (the capture
 * owns it); nothing between bs_record_begin() returning and bs_record_end()
 * may read Backstep's own memory (.backstep section). Software-pended
 * exceptions (PendSV) and SVC are reproduced by replay itself and need no
 * stub. Interrupts are masked inside bs_record_begin/end so that every
 * recorded interrupt lies between the two.
 *
 * SPDX-License-Identifier: MIT
 */
#ifndef BACKSTEP_RECORD_H
#define BACKSTEP_RECORD_H

#include <stdint.h>

#include "backstep_irq.h"
#include "backstep_mmio.h"
#include "backstep_snapshot.h"

#ifdef __cplusplus
extern "C" {
#endif

#define BS_REC_MAGIC 0x43525342u /* "BSRC" little-endian */
#define BS_REC_VERSION 2u /* 2: adds the IRQS chunk (M4) */

/* Chunk types: four ASCII characters, little-endian. */
#define BS_REC_CHUNK_TARGET 0x44544754u /* "TGTD" */
#define BS_REC_CHUNK_SNAPSHOT 0x50414E53u /* "SNAP" */
#define BS_REC_CHUNK_MMIO 0x4F494D4Du   /* "MMIO" */
#define BS_REC_CHUNK_IRQS 0x53515249u   /* "IRQS" */
#define BS_REC_CHUNK_END 0x20444E45u    /* "END " */

/* Target identifiers (the replay tool's memory map and core options). */
#define BS_TARGET_QEMU_MPS2_AN386 1u
#define BS_TARGET_NUCLEO_F411RE 2u

struct bs_record_config {
    uint32_t target_id;
    /* Firmware image in flash (vector table to the end of the .data
     * initialisers); its CRC-32 identifies the build. */
    const void *image_start;
    const void *image_end;
    /* The program's static RAM ([_sdata, _ebss)) and its stack area. The
     * whole stack area, dead part included, goes into the snapshot. */
    const void *ram_start;
    const void *ram_end;
    const void *stack_bottom;
    const void *stack_top;
    /* Working memory, in the .backstep section. */
    struct bs_mmio_event *log;
    uint32_t log_capacity;
    uint8_t *ram_buffer; /* copy of the RAM at the snapshot */
    uint32_t ram_buffer_capacity;
    /* Interrupt positions (ADR 0004): one event per interrupt entered
     * through a BS_IRQ_STUB. May be 0 for programs without interrupts. */
    struct bs_irq_event *irq_log;
    uint32_t irq_capacity;
};

/* Payload of the "END " chunk. Field order is part of the file format. */
struct bs_record_end_state {
    struct bs_cpu_state cpu; /* at the entry of the final bs_snapshot_take */
    uint32_t ram_crc32;      /* CRC-32 of the regions below, in order */
    uint32_t region_count;
    struct bs_region regions[2];
};

/*
 * Start recording: capture peripheral loads, then take the snapshot.
 * Returns 0 when recording (and when replayed), -1 if the core has no MPU.
 * The recording starts where this function returns.
 */
int bs_record_begin(const struct bs_record_config *cfg);

/* End the recording. Replay stops at this point and checks the state. */
void bs_record_end(void);

/*
 * Emit the recording file by calling put() with consecutive pieces.
 * Returns the number of bytes, or 0 if the recording is unusable (no
 * begin/end, the RAM copy or the log overflowed).
 */
uint32_t bs_record_write(void (*put)(const void *data, uint32_t len, void *ctx), void *ctx);

#ifdef __cplusplus
}
#endif

#endif /* BACKSTEP_RECORD_H */
