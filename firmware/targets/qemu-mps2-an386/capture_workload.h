/*
 * Deterministic peripheral-read workload shared by the two capture
 * mechanisms (MPU trap and Clang pass) so their logs can be compared.
 *
 * SPDX-License-Identifier: MIT
 */
#ifndef CAPTURE_WORKLOAD_H
#define CAPTURE_WORKLOAD_H

#include <stdint.h>

/* Reads CMSDK timer/UART registers; returns the number of reads made. */
uint32_t workload_peripheral(uint32_t *checksum);

/* Byte/halfword/word, signed and unsigned reads of a RAM window that is
 * captured as if it were a device. Returns the number of reads made. */
uint32_t workload_ram(uint32_t base, uint32_t *checksum);

/* n word reads of timer RELOAD, for cost measurements. */
uint32_t workload_repeat_reads(uint32_t n);

#endif /* CAPTURE_WORKLOAD_H */
