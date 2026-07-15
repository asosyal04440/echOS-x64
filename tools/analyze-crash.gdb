set pagination off
set confirm off
set architecture i386:x86-64

define crash_report
    echo \n=== CRASH REPORT ===\n
    echo --- Registers ---\n
    info registers
    echo \n--- Backtrace ---\n
    bt full
    echo \n--- Stack Contents ---\n
    x/64gx $rsp
end

define crash_panic_info
    echo \n=== PANIC INFO ===\n
    echo Looking for panic message...\n
    bt
end

echo echOS Crash Analyzer loaded.\n
echo Use: crash_report  — full crash report\n
echo Use: crash_panic_info — panic-specific info\n
