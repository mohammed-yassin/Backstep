/*
 * Included first by every recorder source file. The recorder's code runs
 * inside the firmware's exceptions and interrupt stubs, which replay
 * reproduces without running it; it must therefore never touch the
 * floating-point registers (that would trigger lazy state preservation
 * and change memory replay cannot predict). With GCC on an FPU build
 * this pragma makes the compiler use core registers only;
 * scripts/check-recorder-no-fpu.py checks the built objects.
 *
 * SPDX-License-Identifier: MIT
 */
#ifndef BS_NO_FPU_H
#define BS_NO_FPU_H

#if defined(__arm__) && defined(__ARM_FP) && defined(__GNUC__) && !defined(__clang__)
#pragma GCC target("general-regs-only")
#endif

#endif /* BS_NO_FPU_H */
