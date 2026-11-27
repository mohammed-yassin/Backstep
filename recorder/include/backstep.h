/*
 * Backstep on-device recorder - public API.
 *
 * The recorder is a small, dependency-free C99 library that runs inside the
 * firmware being debugged. Later milestones add the recording API
 * (peripheral reads, interrupt positions, DMA transfers, state snapshots);
 * see docs/ROADMAP.md. Milestone M0 provides versioning and the CRC-32
 * routine that recordings use to checksum CPU and memory state.
 *
 * SPDX-License-Identifier: MIT
 */
#ifndef BACKSTEP_H
#define BACKSTEP_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define BACKSTEP_VERSION_MAJOR 0
#define BACKSTEP_VERSION_MINOR 0
#define BACKSTEP_VERSION_PATCH 1
#define BACKSTEP_VERSION_STRING "0.0.1"

/* Version as 0x00MMmmpp (major, minor, patch). */
uint32_t bs_version(void);

/*
 * CRC-32 (IEEE 802.3: reflected, polynomial 0xEDB88320, initial value and
 * final XOR 0xFFFFFFFF). bs_crc32("123456789", 9) == 0xCBF43926.
 * The host tools implement the identical function; tests keep them in sync.
 */
uint32_t bs_crc32(const void *data, size_t len);

/*
 * Incremental form: start with crc = 0, feed chunks, the result equals the
 * one-shot value for the concatenated data.
 */
uint32_t bs_crc32_update(uint32_t crc, const void *data, size_t len);

#ifdef __cplusplus
}
#endif

#endif /* BACKSTEP_H */
