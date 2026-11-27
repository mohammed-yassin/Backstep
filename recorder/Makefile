# Host-side build and tests for the Backstep recorder library.
#
#   make test   compile with the host C compiler and run the unit tests
#
# The decoder tests need the GNU Arm assembler (arm-none-eabi-as and
# arm-none-eabi-objdump) to generate their reference encodings.
# The same sources are cross-compiled into firmware by firmware/Makefile.
#
# SPDX-License-Identifier: MIT

CC      ?= cc
BUILD   := build
CFLAGS  := -std=c99 -O2 -g -Wall -Wextra -Werror -Wshadow -Wpedantic \
           -Wstrict-prototypes -Wmissing-prototypes -I include
SANITIZE ?= -fsanitize=address,undefined -fno-omit-frame-pointer

.PHONY: test clean
test: $(BUILD)/test_recorder $(BUILD)/test_thumb_ldst
	./$(BUILD)/test_recorder
	./$(BUILD)/test_thumb_ldst

$(BUILD)/test_recorder: tests/test_recorder.c src/backstep.c include/backstep.h
	@mkdir -p $(@D)
	$(CC) $(CFLAGS) $(SANITIZE) -o $@ tests/test_recorder.c src/backstep.c

$(BUILD)/ldst_vectors.h: tests/gen_ldst_vectors.py
	@mkdir -p $(@D)
	python3 tests/gen_ldst_vectors.py $@

$(BUILD)/test_thumb_ldst: tests/test_thumb_ldst.c src/thumb_ldst.c src/thumb_ldst.h $(BUILD)/ldst_vectors.h
	$(CC) $(CFLAGS) -I $(BUILD) $(SANITIZE) -o $@ tests/test_thumb_ldst.c src/thumb_ldst.c

clean:
	rm -rf $(BUILD)
