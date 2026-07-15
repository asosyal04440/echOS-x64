"""echOS GDB initialization script.

Load this in GDB with:
  (gdb) source tools/gdb/init-gdb.py

Or pass it on the command line:
  $ gdb -x tools/gdb/init-gdb.py ./target/debug/echos
"""

import sys
import os

_script_dir = os.path.dirname(os.path.abspath(__file__))
if _script_dir not in sys.path:
    sys.path.insert(0, _script_dir)

import echos_pretty
import echos_commands

echos_pretty.register_printers(gdb)
echos_commands.register_commands()

print("echOS GDB tools loaded: pretty-printers + commands")
print("  Available commands:")
print("    kernel-log       - Display kernel log ring buffer contents")
print("    kernel-backtrace - Enhanced backtrace with kernel-specific info")
print("    kernel-regs      - Display kernel registers with annotations")
print("    dump-page-tables - Walk and display kernel page tables")
print("    kernel-info      - Display kernel version and build info")
