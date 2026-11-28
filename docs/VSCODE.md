# Debugging a recording in VS Code

VS Code debugs through GDB, so a recording is debugged there the same way
as on the command line ([GDB guide](GDB.md)). `backstep gdb` runs as a
GDB server, and a GDB-based debug extension connects to it.

The example files are in [`docs/vscode/`](vscode/): copy `launch.json`
and `tasks.json` into your project's `.vscode/` folder. Then set two
paths: the ELF that was recorded (built with `-g`) and the recording.
The task starts `backstep gdb` in the background. It checks the
recording, waits for GDB, and the debug session attaches. `backstep` and
`gdb-multiarch` (or `arm-none-eabi-gdb`) must be on the `PATH`.

## Cortex-Debug

With [Cortex-Debug](https://marketplace.visualstudio.com/items?itemName=marus25.cortex-debug),
the configuration is an "external" GDB server:

```json
{
    "name": "Backstep: debug recording (Cortex-Debug)",
    "type": "cortex-debug",
    "request": "attach",
    "servertype": "external",
    "gdbTarget": "localhost:3333",
    "executable": "${workspaceFolder}/build/firmware.elf",
    "gdbPath": "gdb-multiarch",
    "preLaunchTask": "backstep gdb",
    "overrideAttachCommands": []
}
```

- **Use `attach`, not `launch`:** a launch would try to flash and reset
  the target, and a recording cannot be written to.
- **Keep the attach commands empty** (`"overrideAttachCommands": []`):
  there is nothing to reset or halt.

## Microsoft C/C++

With [C/C++](https://marketplace.visualstudio.com/items?itemName=ms-vscode.cpptools)
(`cppdbg`), GDB connects through `miDebuggerServerAddress`:

```json
{
    "name": "Backstep: debug recording (C/C++)",
    "type": "cppdbg",
    "request": "launch",
    "program": "${workspaceFolder}/build/firmware.elf",
    "cwd": "${workspaceFolder}",
    "MIMode": "gdb",
    "miDebuggerPath": "gdb-multiarch",
    "miDebuggerServerAddress": "localhost:3333",
    "stopAtConnect": true,
    "preLaunchTask": "backstep gdb"
}
```

## Going backwards

- **Toolbar:** the step, continue and breakpoint buttons work as usual.
  Whether the toolbar offers "Step Back" and "Reverse Continue" depends
  on the extension.
- **Debug Console:** GDB's reverse commands always work there. Prefix
  them with `-exec` in the C/C++ extension:

  ```text
  -exec reverse-continue
  -exec reverse-step
  -exec reverse-finish
  -exec monitor position
  ```

- **Watchpoints** are set from the Watch view or the Debug Console
  (`-exec watch g_rx.limit`). A `reverse-continue` then stops on the
  store that last changed the variable, as in the
  [GDB walkthrough](GDB.md#finding-who-corrupted-a-variable).

## What is tested

- **The GDB side:** CI runs 30 GDB sessions against `backstep gdb`
  (`scripts/gdb-test.py`), over both `target remote` and the TCP port.
  The sessions use the same commands the extensions send.
- **The VS Code extensions themselves:** these are not run in CI. If a
  configuration above does not work with your extension version, please
  open an issue with the extension's debug output. Cortex-Debug shows it
  with `"showDevDebugOutput": "raw"`.
