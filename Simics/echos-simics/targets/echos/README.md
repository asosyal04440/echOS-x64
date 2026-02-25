# echOS Simics Project

This directory contains Simics simulation configuration for echOS testing.

## Directory Structure

```
echos-simics/
├── targets/
│   └── echos/
│       ├── echos-system.simics  # Main system configuration
│       ├── boot.simics          # Boot script
│       ├── debug.simics         # Debug configuration
│       ├── test.simics          # Test suite
│       ├── images/              # Disk images
│       ├── firmware/            # UEFI firmware
│       ├── symbols/             # Debug symbols
│       ├── logs/                # Log files
│       ├── checkpoints/        # Simulation checkpoints
│       ├── data/                # Persistent data
│       └── tests/               # Individual test scripts
```

## Quick Start

### Boot echOS
```simics
run-script %simics%/targets/echos/boot.simics
```

### Debug Mode
```simics
run-script %simics%/targets/echos/debug.simics
```

### Run Tests
```simics
run-script %simics%/targets/echos/test.simics
test-all
```

## System Configuration

- **CPU**: 4 cores @ 3GHz, VMX enabled
- **Memory**: 4GB RAM
- **Storage**: 
  - NVMe 64GB (primary)
  - SATA 128GB (secondary)
- **Network**: 
  - Intel E1000 (eth0)
  - Intel I210 (eth1)
- **USB**: xHCI with keyboard, mouse, storage
- **Graphics**: VGA 1920x1080
- **Audio**: Intel HDA
- **Security**: TPM 2.0

## Debug Features

- JTAG debug on port 5555
- GDB server on port 1234
- Time-travel debugging (reverse execution)
- Memory and I/O tracing
- Breakpoint and watchpoint support

## Test Categories

1. **Boot Tests** - UEFI and kernel initialization
2. **Memory Tests** - Physical memory, page tables, heap
3. **Driver Tests** - NVMe, USB, Network, Audio
4. **Filesystem Tests** - FAT32, ext4, NTFS
5. **Network Tests** - DHCP, TCP, DNS, HTTP
6. **Security Tests** - TPM, Secure Boot, Crypto
7. **Stress Tests** - Memory pressure, CPU load, I/O

## Building Disk Images

1. Create disk image:
   ```bash
   qemu-img create -f raw echos-disk.img 64G
   ```

2. Copy echOS UEFI binary:
   ```bash
   mkdir -p images
   cp ../../target/x86_64-unknown-uefi/debug/ech_os.efi images/bootx64.efi
   ```

3. Create UEFI firmware (OVMF):
   ```bash
   # Download OVMF from your distribution or build from EDK2
   cp /usr/share/OVMF/OVMF_CODE.fd firmware/OVMF.fd
   ```

## Network Configuration

For network connectivity, create a TAP device:
```bash
sudo ip tuntap add dev echos-tap0 mode tap
sudo ip link set echos-tap0 up
sudo ip addr add 10.0.2.1/24 dev echos-tap0
```

## GDB Connection

```bash
gdb -ex 'target remote localhost:1234' \
    -ex 'symbol-file symbols/kernel.sym' \
    -ex 'break kernel_main'
```

## Time Travel Debugging

```simics
# Save checkpoint
$checkpoint.save("before_test")

# Run test
run

# If test fails, go back
$checkpoint.restore("before_test")

# Or use reverse execution
reverse-stepi
reverse-continue
```

## Useful Commands

| Command | Description |
|---------|-------------|
| `run` | Start simulation |
| `stop` | Pause simulation |
| `stepi` | Step one instruction |
| `reverse-stepi` | Step backward |
| `print-state` | Print system state |
| `list-breakpoints` | List all breakpoints |
| `test-all` | Run all tests |

## Troubleshooting

### Boot fails
- Check UEFI firmware path
- Verify disk image exists
- Check serial log for errors

### Network not working
- Verify TAP device is configured
- Check MAC address conflicts
- Enable network tracing for debugging

### GDB connection fails
- Verify GDB server is started
- Check port 1234 is available
- Ensure symbol file matches kernel
