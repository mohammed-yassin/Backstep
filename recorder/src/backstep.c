/*
 * Backstep on-device recorder - core utilities.
 *
 * SPDX-License-Identifier: MIT
 */
#include "backstep.h"

uint32_t bs_version(void)
{
    return ((uint32_t)BACKSTEP_VERSION_MAJOR << 16) |
           ((uint32_t)BACKSTEP_VERSION_MINOR << 8) |
           (uint32_t)BACKSTEP_VERSION_PATCH;
}

/*
 * Nibble-table CRC-32: 64 bytes of table, roughly 2x faster than bitwise
 * and small enough for any microcontroller.
 */
static const uint32_t crc32_nibble_table[16] = {
    0x00000000u, 0x1DB71064u, 0x3B6E20C8u, 0x26D930ACu,
    0x76DC4190u, 0x6B6B51F4u, 0x4DB26158u, 0x5005713Cu,
    0xEDB88320u, 0xF00F9344u, 0xD6D6A3E8u, 0xCB61B38Cu,
    0x9B64C2B0u, 0x86D3D2D4u, 0xA00AE278u, 0xBDBDF21Cu,
};

uint32_t bs_crc32_update(uint32_t crc, const void *data, size_t len)
{
    const uint8_t *p = (const uint8_t *)data;

    crc = ~crc;
    while (len-- != 0u) {
        crc ^= *p++;
        crc = (crc >> 4) ^ crc32_nibble_table[crc & 0xFu];
        crc = (crc >> 4) ^ crc32_nibble_table[crc & 0xFu];
    }
    return ~crc;
}

uint32_t bs_crc32(const void *data, size_t len)
{
    return bs_crc32_update(0u, data, len);
}
