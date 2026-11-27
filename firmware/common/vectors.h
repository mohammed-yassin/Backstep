/*
 * Core exception handler prototypes provided by startup_cm4.c.
 * Firmware overrides a handler simply by defining a function of that name.
 *
 * SPDX-License-Identifier: MIT
 */
#ifndef BACKSTEP_VECTORS_H
#define BACKSTEP_VECTORS_H

void Reset_Handler(void);
void Default_Handler(void);
void NMI_Handler(void);
void HardFault_Handler(void);
void MemManage_Handler(void);
void BusFault_Handler(void);
void UsageFault_Handler(void);
void SVC_Handler(void);
void DebugMon_Handler(void);
void PendSV_Handler(void);
void SysTick_Handler(void);

#endif /* BACKSTEP_VECTORS_H */
