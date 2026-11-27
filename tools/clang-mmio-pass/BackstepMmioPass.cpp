// Clang/LLVM pass plugin: compile-time capture of peripheral reads
// (EXPERIMENTAL, milestone M1 prototype).
//
// Replaces every volatile 8/16/32-bit integer load with a call to
// __bs_mmio_load{8,16,32}(ptr). The runtime (recorder/src/cc_hooks.c)
// performs the load and records it when the address is in the captured
// region. Peripheral registers are always accessed through volatile
// pointers, so this sees every peripheral read the compiler generates.
// It cannot see reads in assembly or in precompiled libraries; see
// docs/adr/0003-peripheral-read-capture.md.
//
// Usage: clang ... -fpass-plugin=libBackstepMmioPass.so
//
// SPDX-License-Identifier: MIT

#include "llvm/IR/IRBuilder.h"
#include "llvm/IR/InstIterator.h"
#include "llvm/IR/Instructions.h"
#include "llvm/IR/Module.h"
#include "llvm/IR/PassManager.h"
#include "llvm/Passes/PassBuilder.h"
#include "llvm/Passes/PassPlugin.h"

using namespace llvm;

namespace {

struct BackstepMmioPass : PassInfoMixin<BackstepMmioPass> {
    PreservedAnalyses run(Function &F, FunctionAnalysisManager &) {
        // Never instrument the runtime hooks themselves.
        if (F.getName().starts_with("__bs_") || F.isDeclaration()) {
            return PreservedAnalyses::all();
        }
        SmallVector<LoadInst *, 16> loads;
        for (Instruction &I : instructions(F)) {
            auto *LI = dyn_cast<LoadInst>(&I);
            if (!LI || !LI->isVolatile() || !LI->getType()->isIntegerTy()) {
                continue;
            }
            unsigned bits = LI->getType()->getIntegerBitWidth();
            if (bits == 8 || bits == 16 || bits == 32) {
                loads.push_back(LI);
            }
        }
        if (loads.empty()) {
            return PreservedAnalyses::all();
        }
        Module &M = *F.getParent();
        for (LoadInst *LI : loads) {
            unsigned bits = LI->getType()->getIntegerBitWidth();
            IRBuilder<> B(LI);
            FunctionCallee hook = M.getOrInsertFunction(
                "__bs_mmio_load" + std::to_string(bits), LI->getType(),
                LI->getPointerOperandType());
            CallInst *call = B.CreateCall(hook, {LI->getPointerOperand()});
            call->setDebugLoc(LI->getDebugLoc());
            LI->replaceAllUsesWith(call);
            LI->eraseFromParent();
        }
        return PreservedAnalyses::none();
    }
    static bool isRequired() { return true; } // run at -O0 too
};

} // namespace

extern "C" LLVM_ATTRIBUTE_WEAK PassPluginLibraryInfo llvmGetPassPluginInfo() {
    return {LLVM_PLUGIN_API_VERSION, "BackstepMmioPass", "0.0.1",
            [](PassBuilder &PB) {
                PB.registerPipelineStartEPCallback(
                    [](ModulePassManager &MPM, OptimizationLevel) {
                        MPM.addPass(
                            createModuleToFunctionPassAdaptor(BackstepMmioPass()));
                    });
            }};
}
