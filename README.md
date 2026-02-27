# echOS-x64
echOS – A minimal operating system written in Rust, starting from UEFI and aiming to run on bare metal. Learning by building.


## Simics Zero-Tolerance Gate

- Day-1 hard-block Simics gate komutu:
	- `Simics\\echos-simics\\bin\\test-runner.bat --zero-tolerance-gate`
- Bu gate 5 eksen için PASS/FAIL üretir:
	- `boot_irq_input`
	- `syscall_security`
	- `fs_network`
	- `performance`
	- `extreme_ironshim`
- Çıktılar:
	- Çalışma logu: `Simics/echos-simics/targets/echos/logs/gate_run_<timestamp>.log`
	- Makine-okunur karar: `Simics/echos-simics/targets/echos/logs/gate_verdict_<timestamp>.json`
- Kural: Tek bir eksen FAIL olursa script `exit code 2` döner ve merge engellenir.
- CI entegrasyonu:
	- Workflow: `.github/workflows/simics-zero-tolerance.yml`
	- Runner etiketi: `self-hosted, windows, simics`
	- PR tetikleyicisinde gate başarısızsa merge block olur.


(This project is still under development and many important parts remain confidential.)
