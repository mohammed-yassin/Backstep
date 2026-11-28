# Backstep example and test firmware.
#
#   make            build every target into build/ (GCC only)
#   make qemu-test  build and run the QEMU tests (exit code = result)
#   make cc-test    M1 capture comparison; also needs clang and LLVM dev
#                   files to build the pass plugin (tools/clang-mmio-pass)
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
           -Wl,--fatal-warnings -Wl,--no-warn-rwx-segments -Wl,-z,noexecstack

COMMON_SRC := common/startup_cm4.c common/libc_min.c $(ROOT)/recorder/src/backstep.c

QEMU_DIR   := targets/qemu-mps2-an386
QEMU_ELF   := $(BUILD)/qemu-mps2-an386/hello.elf
QEMU_TRAP  := $(BUILD)/qemu-mps2-an386/mmio_trap_test.elf
QEMU_IRQ   := $(BUILD)/qemu-mps2-an386/irq_position_test.elf
QEMU_IRQ0  := $(BUILD)/qemu-mps2-an386/irq_position_test_O0.elf
QEMU_SNAP  := $(BUILD)/qemu-mps2-an386/snapshot_test.elf
QEMU_REPLAY := $(BUILD)/qemu-mps2-an386/replay_test.elf
NUCLEO_ELF := $(BUILD)/nucleo-f411re/blinky.elf
NUCLEO_TRAP := $(BUILD)/nucleo-f411re/mmio_trap_hw.elf
NUCLEO_EXCL := $(BUILD)/nucleo-f411re/exclusive_hw.elf
NUCLEO_REC := $(BUILD)/nucleo-f411re/record_hw.elf

TRAP_SRC := $(ROOT)/recorder/src/thumb_ldst.c $(ROOT)/recorder/src/mpu_trap.c
CMP_MPU  := $(BUILD)/qemu-mps2-an386/capture_compare_mpu.elf
CMP_CC   := $(BUILD)/qemu-mps2-an386/capture_compare_cc.elf
WORK_CC  := $(BUILD)/qemu-mps2-an386/capture_workload_cc.o

CLANG       ?= clang
PASS_DIR    := $(ROOT)/tools/clang-mmio-pass
PASS_PLUGIN := $(PASS_DIR)/build/libBackstepMmioPass.so
CLANG_FLAGS := --target=arm-none-eabi -mthumb -mcpu=cortex-m4 -mfloat-abi=soft \
               -std=c99 -O2 -g -Wall -Wextra -Werror -ffreestanding -fno-builtin \
               -fshort-enums \
               -ffunction-sections -fdata-sections

.PHONY: all clean qemu-test cc-test irq-test snapshot-test replay-test
all: $(QEMU_ELF) $(QEMU_TRAP) $(CMP_MPU) $(QEMU_IRQ) $(QEMU_IRQ0) $(QEMU_SNAP) $(QEMU_REPLAY) $(NUCLEO_ELF) $(NUCLEO_ELF:.elf=.bin) $(NUCLEO_ELF:.elf=.hex) \
     $(NUCLEO_TRAP) $(NUCLEO_TRAP:.elf=.bin) $(NUCLEO_EXCL) $(NUCLEO_EXCL:.elf=.bin) \
     $(NUCLEO_REC) $(NUCLEO_REC:.elf=.bin)

$(BUILD)/qemu-mps2-an386/hello.elf: targets/qemu-mps2-an386/hello.c $(COMMON_SRC) targets/qemu-mps2-an386/link.ld common/semihost.h
	@mkdir -p $(@D)
	$(CC) $(CFLAGS) $(LDFLAGS) -T targets/qemu-mps2-an386/link.ld \
	    -Wl,-Map=$(@:.elf=.map) -o $@ targets/qemu-mps2-an386/hello.c $(COMMON_SRC) -lgcc
	$(SIZE) $@

$(QEMU_TRAP): $(QEMU_DIR)/mmio_trap_test.c common/mmio_forms.S $(COMMON_SRC) $(TRAP_SRC) $(QEMU_DIR)/link.ld common/qemu_test.h $(ROOT)/recorder/include/backstep_mmio.h $(ROOT)/recorder/src/thumb_ldst.h
	@mkdir -p $(@D)
	$(CC) $(CFLAGS) -I $(ROOT)/recorder/src $(LDFLAGS) -T $(QEMU_DIR)/link.ld \
	    -Wl,-Map=$(@:.elf=.map) -o $@ $(QEMU_DIR)/mmio_trap_test.c common/mmio_forms.S \
	    $(COMMON_SRC) $(TRAP_SRC) -lgcc
	$(SIZE) $@

$(CMP_MPU): $(QEMU_DIR)/capture_compare.c $(QEMU_DIR)/capture_workload.c $(QEMU_DIR)/capture_workload.h $(COMMON_SRC) $(TRAP_SRC) common/qemu_test.h
	@mkdir -p $(@D)
	$(CC) $(CFLAGS) -I $(ROOT)/recorder/src $(LDFLAGS) -T $(QEMU_DIR)/link.ld \
	    -Wl,-Map=$(@:.elf=.map) -o $@ $(QEMU_DIR)/capture_compare.c $(QEMU_DIR)/capture_workload.c \
	    $(COMMON_SRC) $(TRAP_SRC) -lgcc

$(QEMU_IRQ): $(QEMU_DIR)/irq_position_test.c $(COMMON_SRC) $(ROOT)/recorder/src/irq_record.c $(ROOT)/recorder/include/backstep_irq.h $(QEMU_DIR)/link.ld common/qemu_test.h common/semihost.h
	@mkdir -p $(@D)
	$(CC) $(CFLAGS) $(LDFLAGS) -T $(QEMU_DIR)/link.ld \
	    -Wl,-Map=$(@:.elf=.map) -o $@ $(QEMU_DIR)/irq_position_test.c \
	    $(COMMON_SRC) $(ROOT)/recorder/src/irq_record.c -lgcc
	$(SIZE) $@

# Same test built without optimisation: debug builds keep everything on the
# stack, a different stress for interrupt-position keys.
$(QEMU_IRQ0): $(QEMU_DIR)/irq_position_test.c $(COMMON_SRC) $(ROOT)/recorder/src/irq_record.c $(ROOT)/recorder/include/backstep_irq.h $(QEMU_DIR)/link.ld common/qemu_test.h common/semihost.h
	@mkdir -p $(@D)
	$(CC) $(subst -O2,-O0,$(CFLAGS)) $(LDFLAGS) -T $(QEMU_DIR)/link.ld \
	    -Wl,-Map=$(@:.elf=.map) -o $@ $(QEMU_DIR)/irq_position_test.c \
	    $(COMMON_SRC) $(ROOT)/recorder/src/irq_record.c -lgcc

$(QEMU_SNAP): $(QEMU_DIR)/snapshot_test.c $(COMMON_SRC) $(ROOT)/recorder/src/snapshot.c $(ROOT)/recorder/include/backstep_snapshot.h $(QEMU_DIR)/link.ld common/qemu_test.h common/semihost.h
	@mkdir -p $(@D)
	$(CC) $(CFLAGS) $(LDFLAGS) -T $(QEMU_DIR)/link.ld \
	    -Wl,-Map=$(@:.elf=.map) -o $@ $(QEMU_DIR)/snapshot_test.c \
	    $(COMMON_SRC) $(ROOT)/recorder/src/snapshot.c -lgcc

RECORD_SRC := $(ROOT)/recorder/src/record.c $(ROOT)/recorder/src/snapshot.c $(TRAP_SRC)
$(QEMU_REPLAY): $(QEMU_DIR)/replay_test.c $(COMMON_SRC) $(RECORD_SRC) $(ROOT)/recorder/include/backstep_record.h $(ROOT)/recorder/include/backstep_snapshot.h $(QEMU_DIR)/link.ld common/qemu_test.h common/semihost.h
	@mkdir -p $(@D)
	$(CC) $(CFLAGS) -I $(ROOT)/recorder/src $(LDFLAGS) -T $(QEMU_DIR)/link.ld \
	    -Wl,-Map=$(@:.elf=.map) -o $@ $(QEMU_DIR)/replay_test.c $(COMMON_SRC) $(RECORD_SRC) -lgcc
	$(SIZE) $@

# Needs the host tools: cargo build --release (in host/).
replay-test: $(QEMU_REPLAY)
	python3 $(ROOT)/scripts/replay-test.py --backstep $(ROOT)/host/target/release/backstep \
	    --elf $(QEMU_REPLAY)

snapshot-test: $(QEMU_SNAP)
	$(ROOT)/scripts/snapshot-restore-test.sh $(QEMU_SNAP)

irq-test: $(QEMU_IRQ) $(QEMU_IRQ0)
	$(ROOT)/scripts/irq-position-test.sh $(QEMU_IRQ) $(QEMU_IRQ0)

$(PASS_PLUGIN): $(PASS_DIR)/BackstepMmioPass.cpp $(PASS_DIR)/Makefile
	$(MAKE) -C $(PASS_DIR)

$(WORK_CC): $(QEMU_DIR)/capture_workload.c $(QEMU_DIR)/capture_workload.h $(PASS_PLUGIN)
	@mkdir -p $(@D)
	$(CLANG) $(CLANG_FLAGS) -fpass-plugin=$(PASS_PLUGIN) -c -o $@ $<

$(CMP_CC): $(QEMU_DIR)/capture_compare.c $(WORK_CC) $(COMMON_SRC) $(ROOT)/recorder/src/cc_hooks.c common/qemu_test.h
	@mkdir -p $(@D)
	$(CC) $(CFLAGS) -DUSE_CC $(LDFLAGS) -T $(QEMU_DIR)/link.ld \
	    -Wl,-Map=$(@:.elf=.map) -o $@ $(QEMU_DIR)/capture_compare.c $(WORK_CC) \
	    $(COMMON_SRC) $(ROOT)/recorder/src/cc_hooks.c -lgcc

cc-test: $(CMP_MPU) $(CMP_CC)
	$(ROOT)/scripts/capture-compare.sh $(CMP_MPU) $(CMP_CC) $(WORK_CC)

$(BUILD)/nucleo-f411re/blinky.elf: targets/nucleo-f411re/blinky.c $(COMMON_SRC) targets/nucleo-f411re/link.ld
	@mkdir -p $(@D)
	$(CC) $(CFLAGS) $(LDFLAGS) -T targets/nucleo-f411re/link.ld \
	    -Wl,-Map=$(@:.elf=.map) -o $@ targets/nucleo-f411re/blinky.c $(COMMON_SRC) -lgcc
	$(SIZE) $@

$(NUCLEO_TRAP): targets/nucleo-f411re/mmio_trap_hw.c targets/nucleo-f411re/nucleo_test.h common/mmio_forms.S $(COMMON_SRC) $(TRAP_SRC) targets/nucleo-f411re/link.ld common/test_harness.h
	@mkdir -p $(@D)
	$(CC) $(CFLAGS) -I targets/nucleo-f411re -I $(ROOT)/recorder/src $(LDFLAGS) -T targets/nucleo-f411re/link.ld \
	    -Wl,-Map=$(@:.elf=.map) -o $@ targets/nucleo-f411re/mmio_trap_hw.c common/mmio_forms.S \
	    $(COMMON_SRC) $(TRAP_SRC) -lgcc
	$(SIZE) $@

$(NUCLEO_EXCL): targets/nucleo-f411re/exclusive_hw.c targets/nucleo-f411re/nucleo_test.h $(COMMON_SRC) targets/nucleo-f411re/link.ld common/test_harness.h
	@mkdir -p $(@D)
	$(CC) $(CFLAGS) -I targets/nucleo-f411re $(LDFLAGS) -T targets/nucleo-f411re/link.ld \
	    -Wl,-Map=$(@:.elf=.map) -o $@ targets/nucleo-f411re/exclusive_hw.c $(COMMON_SRC) -lgcc
	$(SIZE) $@

$(NUCLEO_REC): targets/nucleo-f411re/record_hw.c targets/nucleo-f411re/nucleo_test.h $(COMMON_SRC) $(RECORD_SRC) targets/nucleo-f411re/link.ld common/test_harness.h $(ROOT)/recorder/include/backstep_record.h
	@mkdir -p $(@D)
	$(CC) $(CFLAGS) -I targets/nucleo-f411re -I $(ROOT)/recorder/src $(LDFLAGS) -T targets/nucleo-f411re/link.ld \
	    -Wl,-Map=$(@:.elf=.map) -o $@ targets/nucleo-f411re/record_hw.c $(COMMON_SRC) $(RECORD_SRC) -lgcc
	$(SIZE) $@

%.bin: %.elf
	$(OBJCOPY) -O binary $< $@

%.hex: %.elf
	$(OBJCOPY) -O ihex $< $@

qemu-test: $(QEMU_ELF) $(QEMU_TRAP)
	$(ROOT)/scripts/qemu-run.sh $(QEMU_ELF)
	QEMU_EXTRA_ARGS="-icount shift=0" $(ROOT)/scripts/qemu-run.sh $(QEMU_TRAP)
	QEMU_EXTRA_ARGS="-icount shift=0" SEMIHOST_ARG=real-fault $(ROOT)/scripts/qemu-run.sh $(QEMU_TRAP)

clean:
	rm -rf $(BUILD)
