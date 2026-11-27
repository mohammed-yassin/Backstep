#!/usr/bin/env bash
# M1: restore a starting-state snapshot into a fresh machine and check that
# execution continues exactly as it did on the original.
#
#   1. Run snapshot_test.elf normally: it writes snapshot.bin, then the
#      result of a workload that depends on globals, the stack, registers
#      and System Control Space state.
#   2. Start a fresh QEMU halted at reset, load only snapshot.bin through
#      GDB (RAM regions, SCS registers, CPU registers), and resume. The
#      same workload runs and writes its result.
#   3. The results must be identical.
#   4. Negative controls: leaving out the stack, the globals, the
#      registers, or the SCS state must change the result, proving each
#      part of the snapshot is really used.
#
# Usage: snapshot-restore-test.sh <snapshot_test.elf>
#
# SPDX-License-Identifier: MIT
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
elf="$(realpath "$1")"
qemu="${QEMU:-qemu-system-arm}"
gdb="${GDB:-gdb-multiarch}"
work="$(mktemp -d)"
trap 'rm -rf "$work"; [[ -n "${qpid:-}" ]] && kill "$qpid" 2>/dev/null || true' EXIT
cd "$work"
semi="-semihosting-config enable=on,target=native"

timeout 30 "$qemu" -machine mps2-an386 -cpu cortex-m4 -nographic -monitor none \
    -serial stdio $semi -kernel "$elf" >original.txt 2>&1
grep -q '^RESULT PASS' original.txt || { cat original.txt; echo "FAIL original run"; exit 1; }
python3 "$here/snapshot_tool.py" info snapshot.bin
expected="$(od -An -tx4 result_original.bin | tr -d ' ')"
echo "original result:  $expected"

restore() { # $1 = part to omit ("" for none); prints the restored result
    rm -f result_restored.bin
    python3 "$here/snapshot_tool.py" gdb snapshot.bin restore.gdb --elf "$elf" ${1:+--omit "$1"}
    # A free TCP port for QEMU's GDB server (avoids clashes on CI runners).
    local port
    port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')"
    "$qemu" -machine mps2-an386 -cpu cortex-m4 -nographic -monitor none \
        -serial null $semi -kernel "$elf" -S -gdb "tcp:127.0.0.1:$port" \
        >qemu_restore.txt 2>&1 &
    qpid=$!
    for _ in $(seq 1 50); do
        python3 -c "import socket,sys; socket.create_connection(('127.0.0.1', $port), 0.2).close()" \
            2>/dev/null && break
        sleep 0.1
    done
    # gdb drives the restore, then lets the target run. QEMU stays attached
    # to gdb after the firmware's semihosting exit, so wait for the result
    # file instead of for the processes, then stop both.
    timeout 60 "$gdb" -nx -batch -ex "set confirm off" -ex "target remote 127.0.0.1:$port" \
        -x restore.gdb -ex "continue" "$elf" >gdb.txt 2>&1 &
    local gpid=$!
    for _ in $(seq 1 200); do
        [[ -s result_restored.bin ]] && break
        sleep 0.1
    done
    sleep 0.2
    kill "$gpid" "$qpid" 2>/dev/null || true
    wait "$gpid" "$qpid" 2>/dev/null || true
    qpid=""
    if [[ -f result_restored.bin ]]; then
        od -An -tx4 result_restored.bin | tr -d ' '
    else
        echo "none"
    fi
}

got="$(restore "")"
echo "restored result:  $got"
if [[ "$got" != "$expected" ]]; then
    cat gdb.txt; echo "snapshot-restore-test: FAIL (restored run differs)"; exit 1
fi
echo "PASS restored machine continued identically"

for part in region0 region1 regs scs; do
    got="$(restore "$part")"
    if [[ "$got" == "$expected" ]]; then
        echo "snapshot-restore-test: FAIL (result unchanged without $part: the test cannot see it)"
        exit 1
    fi
    echo "PASS leaving out $part changes the result ($got)"
done
echo "snapshot-restore-test: PASS"
