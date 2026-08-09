# WHPX + GDB Kullanım Notları

WHPX (Windows Hypervisor Platform) altında QEMU gdbstub çalışır ancak bazı kısıtlamalar vardır.

## Bilinen Kısıtlamalar

| Özellik | WHPX | TCG |
|---------|------|-----|
| Software breakpoint (INT3) | Çalışır | Çalışır |
| Hardware breakpoint (hbreak) | Önerilir | Çalışır |
| Single step (stepi) | Yavaş | Hızlı |
| Watchpoint | Sınırlı | Çalışır |
| Memory read/write | Çalışır | Çalışır |
| Register access | Çalışır | Çalışır |

## Öneriler

1. WHPX ile debug ederken `hbreak` kullanın, `break` de çalışır ama hbreak daha güvenilirdir
2. Single step yavaşsa `-Accel tcg` ile TCG'ye geçin
3. WHPX + GDB birleşimi genelde kararlıdır
4. Eğer QEMU WHPX'te donarsa, `run_qemu.ps1` otomatik olarak TCG fallback'e geçer

## Kullanım

```powershell
# WHPX ile GDB (varsayılan):
.\run_qemu.ps1 -Gdb -GdbWait

# TCG ile GDB (daha iyi single step):
.\run_qemu.ps1 -Gdb -GdbWait -Accel tcg
```
