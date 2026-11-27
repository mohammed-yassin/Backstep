# Build the Backstep Clang pass plugin. Needs LLVM development files whose
# version matches the clang used for firmware (e.g. Ubuntu llvm-18-dev).
#
# SPDX-License-Identifier: MIT
LLVM_CONFIG ?= llvm-config
CXX := clang++
BUILD := build

$(BUILD)/libBackstepMmioPass.so: BackstepMmioPass.cpp
	@mkdir -p $(BUILD)
	$(CXX) -shared -fPIC -O2 -Wall -Wextra -Werror \
	    $$($(LLVM_CONFIG) --cxxflags | sed "s/-I/-isystem /g") -fno-rtti -o $@ $< \
	    $$($(LLVM_CONFIG) --ldflags)

.PHONY: clean
clean:
	rm -rf $(BUILD)
