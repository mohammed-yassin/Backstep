# Backstep example and test firmware.
#
#   make            build every target into build/
#   make qemu-test  build and run the QEMU self-test (exit code = result)
#   make clean
#
# Requires the Arm GNU toolchain (arm-none-eabi-gcc). QEMU tests also need
# qemu-system-arm.
#
# SPDX-License-Identifier: MIT

CROSS   ?= arm-none-eabi-
CC      := $(CROSS)gcc
OBJCOPY := $(CROSS)objcopy
SIZE    := $(CROSS)size
QEMU    ?= qemu-system-arm

BUILD   := build
ROOT    := ..

# M0 builds soft-float on purpose: the replay emulator gains FPU support in
# milestone M7. See docs/adr/0002-reference-targets.md.
ARCHFLAGS := -mcpu=cortex-m4 -mthumb -mfloat-abi=soft
CFLAGS  := $(ARCHFLAGS) -std=c99 -O2 -g3 -Wall -Wextra -Werror -Wshadow \
           -Wstrict-prototypes -Wmissing-prototypes -ffreestanding \
           -ffunction-sections -fdata-sections \
           -I common -I $(ROOT)/recorder/include
LDFLAGS := $(ARCHFLAGS) -nostdlib -nostartfiles -Wl,--gc-sections \
           -Wl,--fatal-warnings -Wl,--no-warn-rwx-segments

COMMON_SRC := common/startup_cm4.c $(ROOT)/recorder/src/backstep.c

QEMU_ELF   := $(BUILD)/qemu-mps2-an386/hello.elf
NUCLEO_ELF := $(BUILD)/nucleo-f411re/blinky.elf

.PHONY: all clean qemu-test
all: $(QEMU_ELF) $(NUCLEO_ELF) $(NUCLEO_ELF:.elf=.bin) $(NUCLEO_ELF:.elf=.hex)

$(BUILD)/qemu-mps2-an386/hello.elf: targets/qemu-mps2-an386/hello.c $(COMMON_SRC) targets/qemu-mps2-an386/link.ld common/semihost.h
	@mkdir -p $(@D)
	$(CC) $(CFLAGS) $(LDFLAGS) -T targets/qemu-mps2-an386/link.ld \
	    -Wl,-Map=$(@:.elf=.map) -o $@ targets/qemu-mps2-an386/hello.c $(COMMON_SRC) -lgcc
	$(SIZE) $@

$(BUILD)/nucleo-f411re/blinky.elf: targets/nucleo-f411re/blinky.c $(COMMON_SRC) targets/nucleo-f411re/link.ld
	@mkdir -p $(@D)
	$(CC) $(CFLAGS) $(LDFLAGS) -T targets/nucleo-f411re/link.ld \
	    -Wl,-Map=$(@:.elf=.map) -o $@ targets/nucleo-f411re/blinky.c $(COMMON_SRC) -lgcc
	$(SIZE) $@

%.bin: %.elf
	$(OBJCOPY) -O binary $< $@

%.hex: %.elf
	$(OBJCOPY) -O ihex $< $@

qemu-test: $(QEMU_ELF)
	$(ROOT)/scripts/qemu-run.sh $(QEMU_ELF)

clean:
	rm -rf $(BUILD)
