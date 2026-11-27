# Host-side build and tests for the Backstep recorder library.
#
#   make test   compile with the host C compiler and run the unit tests
#
# The same sources are cross-compiled into firmware by firmware/Makefile.
#
# SPDX-License-Identifier: MIT

CC      ?= cc
BUILD   := build
CFLAGS  := -std=c99 -O2 -g -Wall -Wextra -Werror -Wshadow -Wpedantic \
           -Wstrict-prototypes -Wmissing-prototypes -I include
SANITIZE ?= -fsanitize=address,undefined -fno-omit-frame-pointer

.PHONY: test clean
test: $(BUILD)/test_recorder
	./$(BUILD)/test_recorder

$(BUILD)/test_recorder: tests/test_recorder.c src/backstep.c include/backstep.h
	@mkdir -p $(@D)
	$(CC) $(CFLAGS) $(SANITIZE) -o $@ tests/test_recorder.c src/backstep.c

clean:
	rm -rf $(BUILD)
