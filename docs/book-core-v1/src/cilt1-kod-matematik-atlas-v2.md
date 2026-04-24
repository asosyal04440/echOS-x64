# Cilt 1 Kod-Matematik Atlas V2

Bu atlas, cekirdek dosyalardan secilen kod kesitlerini matematiksel model setleriyle birlikte verir.

---

## src/main.rs

- Satir sayisi: 2079
- Derin kesit sayisi: 20

### Kesit 01 (line 93)

```rust
0083: #[cfg(target_os = "uefi")]
0084: const SECURE_BOOT_ENROLL_PENDING_RESET: u8 = 1 << 0;
0085: #[cfg(target_os = "uefi")]
0086: const SECURE_BOOT_ENROLL_FAILED: u8 = 1 << 1;
0087: #[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
0088: const LIMINE_REVISION: u64 = 4;
0089: 
0090: #[cfg(target_os = "uefi")]
0091: #[repr(C)]
0092: #[derive(Clone, Copy)]
0093: struct SecureBootEnrollState {
0094:     magic: u32,
0095:     flags: u8,
0096:     _reserved: [u8; 3],
0097: }
0098: 
0099: #[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
0100: #[used]
0101: #[link_section = ".limine_reqs"]
0102: static LIMINE_BASE_REVISION: [u64; 4] = use_base_revision(LIMINE_REVISION);
0103: 
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 02 (line 158)

```rust
0148:     outb(COM1 + 1, 0x00);
0149:     outb(COM1 + 3, 0x03);
0150:     outb(COM1 + 2, 0xC7);
0151:     outb(COM1 + 4, 0x0B);
0152: }
0153: 
0154: unsafe fn debugcon_write_byte(byte: u8) {
0155:     outb(0xE9, byte);
0156: }
0157: 
0158: fn serial_write_byte(byte: u8) {
0159:     unsafe {
0160:         let mut spins = 1_000_000u32;
0161:         while (inb(COM1 + 5) & 0x20) == 0 {
0162:             if spins == 0 {
0163:                 break;
0164:             }
0165:             spins = spins.saturating_sub(1);
0166:             core::hint::spin_loop();
0167:         }
0168:         outb(COM1, byte);
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 03 (line 172)

```rust
0162:             if spins == 0 {
0163:                 break;
0164:             }
0165:             spins = spins.saturating_sub(1);
0166:             core::hint::spin_loop();
0167:         }
0168:         outb(COM1, byte);
0169:     }
0170: }
0171: 
0172: struct SerialPort;
0173: 
0174: impl Write for SerialPort {
0175:     fn write_str(&mut self, s: &str) -> fmt::Result {
0176:         for byte in s.bytes() {
0177:             serial_write_byte(byte);
0178:         }
0179:         Ok(())
0180:     }
0181: }
0182: 
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 04 (line 175)

```rust
0165:             spins = spins.saturating_sub(1);
0166:             core::hint::spin_loop();
0167:         }
0168:         outb(COM1, byte);
0169:     }
0170: }
0171: 
0172: struct SerialPort;
0173: 
0174: impl Write for SerialPort {
0175:     fn write_str(&mut self, s: &str) -> fmt::Result {
0176:         for byte in s.bytes() {
0177:             serial_write_byte(byte);
0178:         }
0179:         Ok(())
0180:     }
0181: }
0182: 
0183: fn serial_write_str(args: &fmt::Arguments) {
0184:     let mut port = SerialPort;
0185:     let _ = port.write_fmt(*args);
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 05 (line 183)

```rust
0173: 
0174: impl Write for SerialPort {
0175:     fn write_str(&mut self, s: &str) -> fmt::Result {
0176:         for byte in s.bytes() {
0177:             serial_write_byte(byte);
0178:         }
0179:         Ok(())
0180:     }
0181: }
0182: 
0183: fn serial_write_str(args: &fmt::Arguments) {
0184:     let mut port = SerialPort;
0185:     let _ = port.write_fmt(*args);
0186: }
0187: 
0188: fn init_platform_iommu() -> bool {
0189:     let cpu_acpi_ok = ech_os::cpu::acpi::init();
0190:     if cpu_acpi_ok {
0191:         serial_write_str(&format_args!("[SMP] CPU ACPI tables parsed\n"));
0192:     } else {
0193:         serial_write_str(&format_args!(
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 06 (line 188)

```rust
0178:         }
0179:         Ok(())
0180:     }
0181: }
0182: 
0183: fn serial_write_str(args: &fmt::Arguments) {
0184:     let mut port = SerialPort;
0185:     let _ = port.write_fmt(*args);
0186: }
0187: 
0188: fn init_platform_iommu() -> bool {
0189:     let cpu_acpi_ok = ech_os::cpu::acpi::init();
0190:     if cpu_acpi_ok {
0191:         serial_write_str(&format_args!("[SMP] CPU ACPI tables parsed\n"));
0192:     } else {
0193:         serial_write_str(&format_args!(
0194:             "[SMP] CPU ACPI init failed, using CPUID topology\n"
0195:         ));
0196:     }
0197: 
0198:     let iommu_tables_ok = ech_os::memory::init_iommu();
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 07 (line 221)

```rust
0211:         ));
0212:     } else if iommu_tables_ok {
0213:         serial_write_str(&format_args!(
0214:             "[IOMMU] Hardware enable/self-test failed, keeping device init constrained\n"
0215:         ));
0216:     }
0217: 
0218:     iommu_tables_ok && iommu_hw_ok
0219: }
0220: 
0221: fn parse_swap_cmdline(cmdline: &str) -> Option<(u32, u32)> {
0222:     let mut lba: Option<u64> = None;
0223:     let mut slots: Option<u64> = None;
0224:     let mut mb: Option<u64> = None;
0225:     for part in cmdline.split_whitespace() {
0226:         if let Some(value) = part.strip_prefix("swap_lba=") {
0227:             lba = value.parse().ok();
0228:         } else if let Some(value) = part.strip_prefix("swap_slots=") {
0229:             slots = value.parse().ok();
0230:         } else if let Some(value) = part.strip_prefix("swap_mb=") {
0231:             mb = value.parse().ok();
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 08 (line 260)

```rust
0250: unsafe fn debugcon_write_hex(val: u64) {
0251:     let hex = b"0123456789abcdef";
0252:     for i in (0..16).rev() {
0253:         let nibble = ((val >> (i * 4)) & 0xF) as usize;
0254:         debugcon_write_byte(hex[nibble]);
0255:     }
0256:     debugcon_write_byte(b'\n');
0257: }
0258: 
0259: #[panic_handler]
0260: fn panic(info: &core::panic::PanicInfo) -> ! {
0261:     unsafe {
0262:         debugcon_write_byte(b'P');
0263:         debugcon_write_byte(b'\n');
0264: 
0265:         let rbp: u64;
0266:         let rsp: u64;
0267:         core::arch::asm!("mov {}, rbp", out(reg) rbp);
0268:         core::arch::asm!("mov {}, rsp", out(reg) rsp);
0269: 
0270:         debugcon_write_byte(b'R');
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 09 (line 421)

```rust
0411: #[cfg(target_os = "windows")]
0412: #[no_mangle]
0413: pub static boot_lma_end: u8 = 0;
0414: 
0415: #[cfg(target_os = "uefi")]
0416: const PREFERRED_GOP_WIDTH: usize = 1920;
0417: #[cfg(target_os = "uefi")]
0418: const PREFERRED_GOP_HEIGHT: usize = 1080;
0419: 
0420: #[cfg(target_os = "uefi")]
0421: fn gop_mode_rank(width: usize, height: usize, target_width: usize, target_height: usize) -> u8 {
0422:     if width == target_width && height == target_height {
0423:         3
0424:     } else if width >= target_width && height >= target_height {
0425:         2
0426:     } else {
0427:         1
0428:     }
0429: }
0430: 
0431: #[cfg(target_os = "uefi")]
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 10 (line 432)

```rust
0422:     if width == target_width && height == target_height {
0423:         3
0424:     } else if width >= target_width && height >= target_height {
0425:         2
0426:     } else {
0427:         1
0428:     }
0429: }
0430: 
0431: #[cfg(target_os = "uefi")]
0432: fn configure_preferred_gop_mode(gop: &mut GraphicsOutput) {
0433:     let current = gop.current_mode_info().resolution();
0434:     let target = (PREFERRED_GOP_WIDTH, PREFERRED_GOP_HEIGHT);
0435:     let mut best_mode = None;
0436:     let mut best_rank = gop_mode_rank(current.0, current.1, target.0, target.1);
0437:     let mut best_area = current.0.saturating_mul(current.1);
0438:     let mut best_dims = current;
0439: 
0440:     for mode in gop.modes() {
0441:         let dims = mode.info().resolution();
0442:         let rank = gop_mode_rank(dims.0, dims.1, target.0, target.1);
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 11 (line 559)

```rust
0549:     }
0550: 
0551:     loop {
0552:         unsafe {
0553:             asm!("hlt");
0554:         }
0555:     }
0556: }
0557: 
0558: #[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
0559: fn limine_available() -> bool {
0560:     LIMINE_MEMORY_MAP_REQUEST.get_response().is_some()
0561: }
0562: 
0563: #[cfg(target_os = "uefi")]
0564: unsafe fn boot_pipeline_uefi(boot_info_addr: usize, _kaslr_offset: u64) -> ! {
0565:     debugcon_write_byte(b'1'); // Mark: entered boot_pipeline_uefi
0566:                                // Initialize boot safety system FIRST
0567:     ech_os::boot::safety::init();
0568:     debugcon_write_byte(b'2'); // Mark: after safety init
0569:     ech_os::boot::safety::BOOT_SAFETY.enter_phase(ech_os::boot::safety::BootPhase::UefiHandover);
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 12 (line 1393)

```rust
1383:                 cmdline_len,
1384:                 image_size,
1385:                 image_hash,
1386:             },
1387:         );
1388:     }
1389:     kernel_entry(boot_info_ptr as usize, 0, BOOT_MAGIC_UEFI)
1390: }
1391: 
1392: #[cfg(target_os = "uefi")]
1393: fn detect_secure_boot(system_table: &SystemTable<Boot>) -> bool {
1394:     let secure_boot = read_global_u8_variable(system_table, cstr16!("SecureBoot"));
1395:     let setup_mode = read_global_u8_variable(system_table, cstr16!("SetupMode"));
1396:     match (secure_boot, setup_mode) {
1397:         (Some(1), Some(0)) => true,
1398:         _ => false,
1399:     }
1400: }
1401: 
1402: #[cfg(target_os = "uefi")]
1403: fn read_global_u8_variable(system_table: &SystemTable<Boot>, name: &CStr16) -> Option<u8> {
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 13 (line 1403)

```rust
1393: fn detect_secure_boot(system_table: &SystemTable<Boot>) -> bool {
1394:     let secure_boot = read_global_u8_variable(system_table, cstr16!("SecureBoot"));
1395:     let setup_mode = read_global_u8_variable(system_table, cstr16!("SetupMode"));
1396:     match (secure_boot, setup_mode) {
1397:         (Some(1), Some(0)) => true,
1398:         _ => false,
1399:     }
1400: }
1401: 
1402: #[cfg(target_os = "uefi")]
1403: fn read_global_u8_variable(system_table: &SystemTable<Boot>, name: &CStr16) -> Option<u8> {
1404:     let runtime_services = system_table.runtime_services();
1405:     let mut buf = [0u8; 1];
1406:     let vendor = VariableVendor::GLOBAL_VARIABLE;
1407:     match runtime_services.get_variable(name, &vendor, &mut buf) {
1408:         Ok(_) => Some(buf[0]),
1409:         Err(_) => None,
1410:     }
1411: }
1412: 
1413: #[cfg(target_os = "uefi")]
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 14 (line 1414)

```rust
1404:     let runtime_services = system_table.runtime_services();
1405:     let mut buf = [0u8; 1];
1406:     let vendor = VariableVendor::GLOBAL_VARIABLE;
1407:     match runtime_services.get_variable(name, &vendor, &mut buf) {
1408:         Ok(_) => Some(buf[0]),
1409:         Err(_) => None,
1410:     }
1411: }
1412: 
1413: #[cfg(target_os = "uefi")]
1414: fn appliance_variable_vendor() -> VariableVendor {
1415:     VariableVendor(uefi::Guid::new(
1416:         [0x83, 0x61, 0x26, 0x6d],
1417:         [0x25, 0x4b],
1418:         [0xab, 0x49],
1419:         0x8c,
1420:         0x4d,
1421:         [0x74, 0x2f, 0x57, 0x78, 0x62, 0x90],
1422:     ))
1423: }
1424: 
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 15 (line 1426)

```rust
1416:         [0x83, 0x61, 0x26, 0x6d],
1417:         [0x25, 0x4b],
1418:         [0xab, 0x49],
1419:         0x8c,
1420:         0x4d,
1421:         [0x74, 0x2f, 0x57, 0x78, 0x62, 0x90],
1422:     ))
1423: }
1424: 
1425: #[cfg(target_os = "uefi")]
1426: fn read_boot_control_variable_seed(
1427:     system_table: &mut SystemTable<Boot>,
1428: ) -> Option<ech_os::boot::appliance::BootControlBlock> {
1429:     let runtime = system_table.runtime_services();
1430:     let (data, _) = runtime
1431:         .get_variable_boxed(cstr16!("echOSBootControl"), &appliance_variable_vendor())
1432:         .ok()?;
1433:     if data.len() != core::mem::size_of::<ech_os::boot::appliance::BootControlBlock>() {
1434:         return None;
1435:     }
1436:     let block = unsafe { *(data.as_ptr() as *const ech_os::boot::appliance::BootControlBlock) };
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 16 (line 1441)

```rust
1431:         .get_variable_boxed(cstr16!("echOSBootControl"), &appliance_variable_vendor())
1432:         .ok()?;
1433:     if data.len() != core::mem::size_of::<ech_os::boot::appliance::BootControlBlock>() {
1434:         return None;
1435:     }
1436:     let block = unsafe { *(data.as_ptr() as *const ech_os::boot::appliance::BootControlBlock) };
1437:     block.validate().then_some(block)
1438: }
1439: 
1440: #[cfg(target_os = "uefi")]
1441: fn curated_app_bundle_path(index: u8) -> Option<&'static CStr16> {
1442:     match index {
1443:         1 => Some(cstr16!("EFI\\BOOT\\APP0001.BHD")),
1444:         2 => Some(cstr16!("EFI\\BOOT\\APP0002.BHD")),
1445:         3 => Some(cstr16!("EFI\\BOOT\\APP0003.BHD")),
1446:         4 => Some(cstr16!("EFI\\BOOT\\APP0004.BHD")),
1447:         5 => Some(cstr16!("EFI\\BOOT\\APP0005.BHD")),
1448:         6 => Some(cstr16!("EFI\\BOOT\\APP0006.BHD")),
1449:         7 => Some(cstr16!("EFI\\BOOT\\APP0007.BHD")),
1450:         8 => Some(cstr16!("EFI\\BOOT\\APP0008.BHD")),
1451:         9 => Some(cstr16!("EFI\\BOOT\\APP0009.BHD")),
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 17 (line 1480)

```rust
1470:         28 => Some(cstr16!("EFI\\BOOT\\APP0028.BHD")),
1471:         29 => Some(cstr16!("EFI\\BOOT\\APP0029.BHD")),
1472:         30 => Some(cstr16!("EFI\\BOOT\\APP0030.BHD")),
1473:         31 => Some(cstr16!("EFI\\BOOT\\APP0031.BHD")),
1474:         32 => Some(cstr16!("EFI\\BOOT\\APP0032.BHD")),
1475:         _ => None,
1476:     }
1477: }
1478: 
1479: #[cfg(target_os = "uefi")]
1480: fn read_efi_boot_file(
1481:     system_table: &mut SystemTable<Boot>,
1482:     image: Handle,
1483:     path: &CStr16,
1484: ) -> Option<Vec<u8>> {
1485:     let boot_services = system_table.boot_services();
1486:     let loaded_image = boot_services
1487:         .open_protocol_exclusive::<LoadedImage>(image)
1488:         .ok()?;
1489:     let mut fs = boot_services
1490:         .open_protocol_exclusive::<SimpleFileSystem>(loaded_image.device())
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 18 (line 1514)

```rust
1504:     let mut raw = vec![0u8; file_size];
1505:     let len = file.read(&mut raw).ok()?;
1506:     if len == 0 {
1507:         return None;
1508:     }
1509:     raw.truncate(len);
1510:     Some(raw)
1511: }
1512: 
1513: #[cfg(target_os = "uefi")]
1514: fn efi_boot_file_size(
1515:     system_table: &mut SystemTable<Boot>,
1516:     image: Handle,
1517:     path: &CStr16,
1518: ) -> Option<usize> {
1519:     let boot_services = system_table.boot_services();
1520:     let loaded_image = boot_services
1521:         .open_protocol_exclusive::<LoadedImage>(image)
1522:         .ok()?;
1523:     let mut fs = boot_services
1524:         .open_protocol_exclusive::<SimpleFileSystem>(loaded_image.device())
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 19 (line 1538)

```rust
1528:         .open(path, FileMode::Read, FileAttribute::empty())
1529:         .ok()?;
1530:     let mut file = handle.into_regular_file()?;
1531:     let info = file
1532:         .get_boxed_info::<uefi::proto::media::file::FileInfo>()
1533:         .ok()?;
1534:     Some(info.file_size() as usize)
1535: }
1536: 
1537: #[cfg(target_os = "uefi")]
1538: fn read_boot_control_seed(
1539:     system_table: &mut SystemTable<Boot>,
1540:     image: Handle,
1541: ) -> Option<ech_os::boot::appliance::BootControlBlock> {
1542:     let mut raw = read_efi_boot_file(system_table, image, cstr16!("EFI\\BOOT\\BOOTCTRL.BIN"))?;
1543:     if raw.len() != core::mem::size_of::<ech_os::boot::appliance::BootControlBlock>() {
1544:         return None;
1545:     }
1546:     let block = unsafe { *(raw.as_mut_ptr() as *const ech_os::boot::appliance::BootControlBlock) };
1547:     block.validate().then_some(block)
1548: }
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 20 (line 1551)

```rust
1541: ) -> Option<ech_os::boot::appliance::BootControlBlock> {
1542:     let mut raw = read_efi_boot_file(system_table, image, cstr16!("EFI\\BOOT\\BOOTCTRL.BIN"))?;
1543:     if raw.len() != core::mem::size_of::<ech_os::boot::appliance::BootControlBlock>() {
1544:         return None;
1545:     }
1546:     let block = unsafe { *(raw.as_mut_ptr() as *const ech_os::boot::appliance::BootControlBlock) };
1547:     block.validate().then_some(block)
1548: }
1549: 
1550: #[cfg(target_os = "uefi")]
1551: fn sync_boot_control_seed(
1552:     system_table: &mut SystemTable<Boot>,
1553:     image: Handle,
1554:     block: &ech_os::boot::appliance::BootControlBlock,
1555: ) {
1556:     let runtime = system_table.runtime_services();
1557:     let attributes = uefi::table::runtime::VariableAttributes::BOOTSERVICE_ACCESS
1558:         | uefi::table::runtime::VariableAttributes::RUNTIME_ACCESS
1559:         | uefi::table::runtime::VariableAttributes::NON_VOLATILE;
1560:     let bytes = unsafe {
1561:         core::slice::from_raw_parts(
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

---

## src/task/scheduler.rs

- Satir sayisi: 1948
- Derin kesit sayisi: 20

### Kesit 01 (line 80)

```rust
0070: static mut STEALERS: Vec<Option<Stealer<Task>>> = Vec::new();
0071: 
0072: // Global görev ID sayacı
0073: static NEXT_TASK_ID: AtomicUsize = AtomicUsize::new(1);
0074: 
0075: // ============================================================================
0076: // SMP-AWARE SCHEDULER YAPISI (CHASE-LEV LOCK-FREE WORK STEALING)
0077: // ============================================================================
0078: 
0079: /// Global SMP scheduler yapısı (Legacy wrapper)
0080: pub struct SmpScheduler {
0081:     cpu_count: AtomicU32,
0082: }
0083: 
0084: impl SmpScheduler {
0085:     pub fn new(cpu_count: u32) -> Self {
0086:         Self {
0087:             cpu_count: AtomicU32::new(cpu_count),
0088:         }
0089:     }
0090: 
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 02 (line 85)

```rust
0075: // ============================================================================
0076: // SMP-AWARE SCHEDULER YAPISI (CHASE-LEV LOCK-FREE WORK STEALING)
0077: // ============================================================================
0078: 
0079: /// Global SMP scheduler yapısı (Legacy wrapper)
0080: pub struct SmpScheduler {
0081:     cpu_count: AtomicU32,
0082: }
0083: 
0084: impl SmpScheduler {
0085:     pub fn new(cpu_count: u32) -> Self {
0086:         Self {
0087:             cpu_count: AtomicU32::new(cpu_count),
0088:         }
0089:     }
0090: 
0091:     pub fn allocate_task_id(&self) -> TaskId {
0092:         NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed)
0093:     }
0094: 
0095:     pub fn spawn(&self, task: Task) {
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 03 (line 91)

```rust
0081:     cpu_count: AtomicU32,
0082: }
0083: 
0084: impl SmpScheduler {
0085:     pub fn new(cpu_count: u32) -> Self {
0086:         Self {
0087:             cpu_count: AtomicU32::new(cpu_count),
0088:         }
0089:     }
0090: 
0091:     pub fn allocate_task_id(&self) -> TaskId {
0092:         NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed)
0093:     }
0094: 
0095:     pub fn spawn(&self, task: Task) {
0096:         let task = Box::new(task);
0097:         let target_cpu = choose_spawn_cpu(&task);
0098:         if let Some(actual_cpu) = enqueue_boxed_task(target_cpu, task) {
0099:             publish_worker_load(actual_cpu);
0100:         } else {
0101:             crate::serial_println!("ERROR: No workers available to spawn task!");
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 04 (line 95)

```rust
0085:     pub fn new(cpu_count: u32) -> Self {
0086:         Self {
0087:             cpu_count: AtomicU32::new(cpu_count),
0088:         }
0089:     }
0090: 
0091:     pub fn allocate_task_id(&self) -> TaskId {
0092:         NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed)
0093:     }
0094: 
0095:     pub fn spawn(&self, task: Task) {
0096:         let task = Box::new(task);
0097:         let target_cpu = choose_spawn_cpu(&task);
0098:         if let Some(actual_cpu) = enqueue_boxed_task(target_cpu, task) {
0099:             publish_worker_load(actual_cpu);
0100:         } else {
0101:             crate::serial_println!("ERROR: No workers available to spawn task!");
0102:         }
0103:     }
0104: 
0105:     // Zaten box'lanmış görevler için dahili yardımcı (örn. timer'dan gelen görevler)
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 05 (line 106)

```rust
0096:         let task = Box::new(task);
0097:         let target_cpu = choose_spawn_cpu(&task);
0098:         if let Some(actual_cpu) = enqueue_boxed_task(target_cpu, task) {
0099:             publish_worker_load(actual_cpu);
0100:         } else {
0101:             crate::serial_println!("ERROR: No workers available to spawn task!");
0102:         }
0103:     }
0104: 
0105:     // Zaten box'lanmış görevler için dahili yardımcı (örn. timer'dan gelen görevler)
0106:     pub fn spawn_boxed(&self, task: Box<Task>) {
0107:         let target_cpu = choose_spawn_cpu(&task);
0108:         if let Some(actual_cpu) = enqueue_boxed_task(target_cpu, task) {
0109:             publish_worker_load(actual_cpu);
0110:         } else {
0111:             crate::serial_println!("ERROR: No workers available to spawn task!");
0112:         }
0113:     }
0114: }
0115: 
0116: fn task_can_run_on_cpu(task: &Task, cpu_id: u32) -> bool {
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 06 (line 116)

```rust
0106:     pub fn spawn_boxed(&self, task: Box<Task>) {
0107:         let target_cpu = choose_spawn_cpu(&task);
0108:         if let Some(actual_cpu) = enqueue_boxed_task(target_cpu, task) {
0109:             publish_worker_load(actual_cpu);
0110:         } else {
0111:             crate::serial_println!("ERROR: No workers available to spawn task!");
0112:         }
0113:     }
0114: }
0115: 
0116: fn task_can_run_on_cpu(task: &Task, cpu_id: u32) -> bool {
0117:     task.hot.affinity == 0xFFFF_FFFF || (cpu_id < 32 && (task.hot.affinity & (1u32 << cpu_id)) != 0)
0118: }
0119: 
0120: fn queued_task_count_usize(cpu_id: usize) -> u32 {
0121:     unsafe {
0122:         WORKERS
0123:             .get(cpu_id)
0124:             .and_then(|w| w.as_ref())
0125:             .map(|worker| worker.len() as u32)
0126:             .unwrap_or(0)
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 07 (line 120)

```rust
0110:         } else {
0111:             crate::serial_println!("ERROR: No workers available to spawn task!");
0112:         }
0113:     }
0114: }
0115: 
0116: fn task_can_run_on_cpu(task: &Task, cpu_id: u32) -> bool {
0117:     task.hot.affinity == 0xFFFF_FFFF || (cpu_id < 32 && (task.hot.affinity & (1u32 << cpu_id)) != 0)
0118: }
0119: 
0120: fn queued_task_count_usize(cpu_id: usize) -> u32 {
0121:     unsafe {
0122:         WORKERS
0123:             .get(cpu_id)
0124:             .and_then(|w| w.as_ref())
0125:             .map(|worker| worker.len() as u32)
0126:             .unwrap_or(0)
0127:     }
0128: }
0129: 
0130: pub fn queued_task_count(cpu_id: u32) -> u32 {
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 08 (line 130)

```rust
0120: fn queued_task_count_usize(cpu_id: usize) -> u32 {
0121:     unsafe {
0122:         WORKERS
0123:             .get(cpu_id)
0124:             .and_then(|w| w.as_ref())
0125:             .map(|worker| worker.len() as u32)
0126:             .unwrap_or(0)
0127:     }
0128: }
0129: 
0130: pub fn queued_task_count(cpu_id: u32) -> u32 {
0131:     queued_task_count_usize(cpu_id as usize)
0132: }
0133: 
0134: fn publish_worker_load(cpu_id: usize) {
0135:     crate::cpu::smp::update_cpu_load(cpu_id as u32, queued_task_count_usize(cpu_id));
0136: }
0137: 
0138: fn choose_spawn_cpu(task: &Task) -> usize {
0139:     let current_cpu = get_current_cpu_id() as usize;
0140:     let cpu_limit = SMP_SCHEDULER.cpu_count.load(Ordering::Relaxed).max(1) as usize;
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 09 (line 134)

```rust
0124:             .and_then(|w| w.as_ref())
0125:             .map(|worker| worker.len() as u32)
0126:             .unwrap_or(0)
0127:     }
0128: }
0129: 
0130: pub fn queued_task_count(cpu_id: u32) -> u32 {
0131:     queued_task_count_usize(cpu_id as usize)
0132: }
0133: 
0134: fn publish_worker_load(cpu_id: usize) {
0135:     crate::cpu::smp::update_cpu_load(cpu_id as u32, queued_task_count_usize(cpu_id));
0136: }
0137: 
0138: fn choose_spawn_cpu(task: &Task) -> usize {
0139:     let current_cpu = get_current_cpu_id() as usize;
0140:     let cpu_limit = SMP_SCHEDULER.cpu_count.load(Ordering::Relaxed).max(1) as usize;
0141:     let mut best_cpu = current_cpu.min(cpu_limit.saturating_sub(1));
0142:     let mut best_load = queued_task_count_usize(best_cpu);
0143: 
0144:     for cpu in 0..cpu_limit {
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 10 (line 138)

```rust
0128: }
0129: 
0130: pub fn queued_task_count(cpu_id: u32) -> u32 {
0131:     queued_task_count_usize(cpu_id as usize)
0132: }
0133: 
0134: fn publish_worker_load(cpu_id: usize) {
0135:     crate::cpu::smp::update_cpu_load(cpu_id as u32, queued_task_count_usize(cpu_id));
0136: }
0137: 
0138: fn choose_spawn_cpu(task: &Task) -> usize {
0139:     let current_cpu = get_current_cpu_id() as usize;
0140:     let cpu_limit = SMP_SCHEDULER.cpu_count.load(Ordering::Relaxed).max(1) as usize;
0141:     let mut best_cpu = current_cpu.min(cpu_limit.saturating_sub(1));
0142:     let mut best_load = queued_task_count_usize(best_cpu);
0143: 
0144:     for cpu in 0..cpu_limit {
0145:         let cpu_id = cpu as u32;
0146:         if !cpu_slots::is_online(cpu_id) || !task_can_run_on_cpu(task, cpu_id) {
0147:             continue;
0148:         }
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 11 (line 165)

```rust
0155:         let load = queued_task_count_usize(cpu);
0156:         if load < best_load || (load == best_load && cpu == current_cpu) {
0157:             best_cpu = cpu;
0158:             best_load = load;
0159:         }
0160:     }
0161: 
0162:     best_cpu
0163: }
0164: 
0165: fn enqueue_boxed_task(target_cpu: usize, task: Box<Task>) -> Option<usize> {
0166:     unsafe {
0167:         if let Some(worker) = WORKERS.get(target_cpu).and_then(|w| w.as_ref()) {
0168:             worker.push(task);
0169:             Some(target_cpu)
0170:         } else if let Some(worker) = WORKERS.get(0).and_then(|w| w.as_ref()) {
0171:             worker.push(task);
0172:             Some(0)
0173:         } else {
0174:             None
0175:         }
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 12 (line 215)

```rust
0205: static SCHEDULER_READY: AtomicBool = AtomicBool::new(false);
0206: static SECONDARY_SCHEDULING_ACTIVE: AtomicBool = AtomicBool::new(false);
0207: 
0208: const NICE_0_LOAD: u64 = 1024;
0209: const SCHED_LATENCY_TICKS: u64 = 20;
0210: const MIN_GRANULARITY_TICKS: u64 = 4;
0211: const LOAD_BALANCE_INTERVAL: usize = 100;
0212: const VRUNTIME_NORMALIZE_INTERVAL: usize = 2000;
0213: 
0214: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0215: enum SchedulerPressureClass {
0216:     Normal,
0217:     Elevated,
0218:     Critical,
0219: }
0220: 
0221: #[derive(Clone, Copy, Debug)]
0222: struct SchedulerPressureSnapshot {
0223:     class: SchedulerPressureClass,
0224:     memory_some_avg10: u64,
0225:     memory_full_avg10: u64,
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 13 (line 222)

```rust
0212: const VRUNTIME_NORMALIZE_INTERVAL: usize = 2000;
0213: 
0214: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0215: enum SchedulerPressureClass {
0216:     Normal,
0217:     Elevated,
0218:     Critical,
0219: }
0220: 
0221: #[derive(Clone, Copy, Debug)]
0222: struct SchedulerPressureSnapshot {
0223:     class: SchedulerPressureClass,
0224:     memory_some_avg10: u64,
0225:     memory_full_avg10: u64,
0226: }
0227: 
0228: // ============================================================================
0229: // PUBLIC API
0230: // ============================================================================
0231: 
0232: /// Scheduler'ı başlatır.
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 14 (line 233)

```rust
0223:     class: SchedulerPressureClass,
0224:     memory_some_avg10: u64,
0225:     memory_full_avg10: u64,
0226: }
0227: 
0228: // ============================================================================
0229: // PUBLIC API
0230: // ============================================================================
0231: 
0232: /// Scheduler'ı başlatır.
0233: pub fn init() {
0234:     // Zaten başlatılmış mı kontrol et (örn. smp::init tarafından update_cpu_count çağrıldıysa)
0235:     unsafe {
0236:         if !PER_CPU_IDLE_TASK.is_empty() {
0237:             crate::serial_println!("SMP Scheduler already initialized, skipping");
0238:             return;
0239:         }
0240:     }
0241: 
0242:     // CPU sayısını al (başlangıçta 1, SMP başlatılınca güncellenecek)
0243:     let cpu_count = crate::cpu::CPU_INFO
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 15 (line 282)

```rust
0272:     crate::serial_println!(
0273:         "SMP Scheduler initialized for {} CPUs (Chase-Lev)",
0274:         cpu_count
0275:     );
0276:     crate::random::init(get_ticks() as u32 + 0xDEADBEEF);
0277:     SECONDARY_SCHEDULING_ACTIVE.store(false, Ordering::Release);
0278:     SCHEDULER_READY.store(true, Ordering::Release);
0279: }
0280: 
0281: /// Belirli bir CPU'nun yük istatistiklerini döndür
0282: pub fn get_cpu_load(cpu_id: u32) -> f32 {
0283:     unsafe {
0284:         if let Some(worker) = WORKERS.get(cpu_id as usize).and_then(|w| w.as_ref()) {
0285:             // Worker kuyruğundaki görev sayısına göre yük tahmini
0286:             let queue_len = worker.len() as f32;
0287:             // Normalize: 0-10 arası kuyruk → 0-100%
0288:             (queue_len / 10.0 * 100.0).min(100.0)
0289:         } else {
0290:             0.0
0291:         }
0292:     }
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 16 (line 296)

```rust
0286:             let queue_len = worker.len() as f32;
0287:             // Normalize: 0-10 arası kuyruk → 0-100%
0288:             (queue_len / 10.0 * 100.0).min(100.0)
0289:         } else {
0290:             0.0
0291:         }
0292:     }
0293: }
0294: 
0295: /// SMP için CPU sayısını güncelle
0296: pub fn update_cpu_count(cpu_count: u32) {
0297:     let cpu_count = cpu_count.min(MAX_CPUS as u32);
0298:     SMP_SCHEDULER.cpu_count.store(cpu_count, Ordering::Relaxed);
0299: 
0300:     unsafe {
0301:         if PER_CPU_CURRENT_TASK.len() < cpu_count as usize {
0302:             for cpu_id in PER_CPU_CURRENT_TASK.len() as u32..cpu_count {
0303:                 PER_CPU_CURRENT_TASK.push(None);
0304:                 PER_CPU_IDLE_TASK.push(Box::new(Task::idle_with_cpu(cpu_id)));
0305:                 PER_CPU_DUMMY_CONTEXT.push(TaskContext::new(0, 0));
0306: 
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 17 (line 318)

```rust
0308:                 WORKERS.push(Some(w));
0309:                 STEALERS.push(Some(s));
0310:             }
0311:         }
0312:     }
0313: 
0314:     crate::serial_println!("Scheduler updated for {} CPUs", cpu_count);
0315:     SCHEDULER_READY.store(true, Ordering::Release);
0316: }
0317: 
0318: pub fn enable_secondary_scheduling() {
0319:     SECONDARY_SCHEDULING_ACTIVE.store(true, Ordering::Release);
0320: }
0321: 
0322: pub fn secondary_scheduling_active() -> bool {
0323:     SECONDARY_SCHEDULING_ACTIVE.load(Ordering::Acquire)
0324: }
0325: 
0326: pub fn current_kernel_stack_top() -> u64 {
0327:     let cpu_id = get_current_cpu_id();
0328:     unsafe {
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 18 (line 322)

```rust
0312:     }
0313: 
0314:     crate::serial_println!("Scheduler updated for {} CPUs", cpu_count);
0315:     SCHEDULER_READY.store(true, Ordering::Release);
0316: }
0317: 
0318: pub fn enable_secondary_scheduling() {
0319:     SECONDARY_SCHEDULING_ACTIVE.store(true, Ordering::Release);
0320: }
0321: 
0322: pub fn secondary_scheduling_active() -> bool {
0323:     SECONDARY_SCHEDULING_ACTIVE.load(Ordering::Acquire)
0324: }
0325: 
0326: pub fn current_kernel_stack_top() -> u64 {
0327:     let cpu_id = get_current_cpu_id();
0328:     unsafe {
0329:         if let Some(task) = PER_CPU_CURRENT_TASK
0330:             .get(cpu_id as usize)
0331:             .and_then(|t| t.as_ref())
0332:         {
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 19 (line 326)

```rust
0316: }
0317: 
0318: pub fn enable_secondary_scheduling() {
0319:     SECONDARY_SCHEDULING_ACTIVE.store(true, Ordering::Release);
0320: }
0321: 
0322: pub fn secondary_scheduling_active() -> bool {
0323:     SECONDARY_SCHEDULING_ACTIVE.load(Ordering::Acquire)
0324: }
0325: 
0326: pub fn current_kernel_stack_top() -> u64 {
0327:     let cpu_id = get_current_cpu_id();
0328:     unsafe {
0329:         if let Some(task) = PER_CPU_CURRENT_TASK
0330:             .get(cpu_id as usize)
0331:             .and_then(|t| t.as_ref())
0332:         {
0333:             task.kernel_stack_top
0334:         } else {
0335:             PER_CPU_IDLE_TASK
0336:                 .get(cpu_id as usize)
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 20 (line 343)

```rust
0333:             task.kernel_stack_top
0334:         } else {
0335:             PER_CPU_IDLE_TASK
0336:                 .get(cpu_id as usize)
0337:                 .map(|t| t.kernel_stack_top)
0338:                 .unwrap_or(0)
0339:         }
0340:     }
0341: }
0342: 
0343: pub fn classify_current_kernel_stack_fault(addr: u64) -> Option<&'static str> {
0344:     let cpu_id = get_current_cpu_id();
0345:     unsafe {
0346:         let task = PER_CPU_CURRENT_TASK
0347:             .get(cpu_id as usize)
0348:             .and_then(|t| t.as_ref())?;
0349:         if addr >= task.hot.kernel_stack_guard_base && addr < task.hot.kernel_stack_bottom {
0350:             Some("KERNEL_STACK_GUARD_PAGE")
0351:         } else {
0352:             None
0353:         }
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

---

## src/task/rt_scheduler.rs

- Satir sayisi: 628
- Derin kesit sayisi: 20

### Kesit 01 (line 81)

```rust
0071: /// SCHED_RR için minimum zaman dilimi
0072: pub const RR_MIN_TIMESLICE: u64 = 10;
0073: 
0074: // ============================================================================
0075: // ZAMANLAMA POLİTİKASI
0076: // ============================================================================
0077: 
0078: /// Zamanlama politika türleri (Linux ile uyumlu sayısal kodlar)
0079: #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
0080: #[repr(u8)]
0081: pub enum SchedPolicy {
0082:     /// Normal zamanlama (CFS benzeri, nice değerine göre)
0083:     Normal = 0,
0084:     /// İlk Giren İlk Çıkar gerçek zamanlı (bloke/yield'e kadar çalışır)
0085:     Fifo = 1,
0086:     /// Round-Robin gerçek zamanlı (zaman dilimi dolunca sıraya girer)
0087:     RoundRobin = 2,
0088:     /// Son tarih bazlı zamanlama (EDF — Earliest Deadline First)
0089:     Deadline = 3,
0090:     /// Boşta zamanlama (çok düşük öncelik, sadece CPU boşta iken çalışır)
0091:     Idle = 4,
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 02 (line 97)

```rust
0087:     RoundRobin = 2,
0088:     /// Son tarih bazlı zamanlama (EDF — Earliest Deadline First)
0089:     Deadline = 3,
0090:     /// Boşta zamanlama (çok düşük öncelik, sadece CPU boşta iken çalışır)
0091:     Idle = 4,
0092:     /// Toplu işlem (CPU yoğun, etkileşim gecikme toleransı var)
0093:     Batch = 5,
0094: }
0095: 
0096: impl Default for SchedPolicy {
0097:     fn default() -> Self {
0098:         SchedPolicy::Normal
0099:     }
0100: }
0101: 
0102: /// Gerçek zamanlı zamanlama parametreleri (sched_param yapısına karşılık gelir)
0103: #[derive(Debug, Clone, Copy)]
0104: pub struct RtSchedParam {
0105:     /// Gerçek zamanlı öncelik (1-99, yüksek = daha önemli)
0106:     pub sched_priority: i32,
0107:     /// SCHED_DEADLINE için: nanosaniye cinsinden çalışma bütçesi
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 03 (line 104)

```rust
0094: }
0095: 
0096: impl Default for SchedPolicy {
0097:     fn default() -> Self {
0098:         SchedPolicy::Normal
0099:     }
0100: }
0101: 
0102: /// Gerçek zamanlı zamanlama parametreleri (sched_param yapısına karşılık gelir)
0103: #[derive(Debug, Clone, Copy)]
0104: pub struct RtSchedParam {
0105:     /// Gerçek zamanlı öncelik (1-99, yüksek = daha önemli)
0106:     pub sched_priority: i32,
0107:     /// SCHED_DEADLINE için: nanosaniye cinsinden çalışma bütçesi
0108:     pub sched_runtime: u64,
0109:     /// SCHED_DEADLINE için: nanosaniye cinsinden son tarih
0110:     pub sched_deadline: u64,
0111:     /// SCHED_DEADLINE için: nanosaniye cinsinden periyot
0112:     pub sched_period: u64,
0113: }
0114: 
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 04 (line 116)

```rust
0106:     pub sched_priority: i32,
0107:     /// SCHED_DEADLINE için: nanosaniye cinsinden çalışma bütçesi
0108:     pub sched_runtime: u64,
0109:     /// SCHED_DEADLINE için: nanosaniye cinsinden son tarih
0110:     pub sched_deadline: u64,
0111:     /// SCHED_DEADLINE için: nanosaniye cinsinden periyot
0112:     pub sched_period: u64,
0113: }
0114: 
0115: impl Default for RtSchedParam {
0116:     fn default() -> Self {
0117:         Self {
0118:             sched_priority: 0,
0119:             sched_runtime: 0,
0120:             sched_deadline: 0,
0121:             sched_period: 0,
0122:         }
0123:     }
0124: }
0125: 
0126: // ============================================================================
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 05 (line 132)

```rust
0122:         }
0123:     }
0124: }
0125: 
0126: // ============================================================================
0127: // GERÇEK ZAMANLI GÖREV BİLGİSİ
0128: // ============================================================================
0129: 
0130: /// Gerçek zamanlı görevin izleme bilgisi
0131: #[derive(Debug, Clone)]
0132: pub struct RtTaskInfo {
0133:     pub task_id: TaskId,
0134:     pub policy: SchedPolicy,
0135:     pub priority: i32,
0136:     /// Kalan zaman dilimi (SCHED_RR için; her tick azalır)
0137:     pub time_slice: u64,
0138:     /// Toplam zaman dilimi (SCHED_RR için başlangıç değeri)
0139:     pub total_timeslice: u64,
0140:     /// CPU yakınlık maskesi (hangi CPU'larda çalışabilir)
0141:     pub affinity: u64,
0142:     /// Bu görev gerçek zamanlı mı? (FIFO veya RR politikası)
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 06 (line 147)

```rust
0137:     pub time_slice: u64,
0138:     /// Toplam zaman dilimi (SCHED_RR için başlangıç değeri)
0139:     pub total_timeslice: u64,
0140:     /// CPU yakınlık maskesi (hangi CPU'larda çalışabilir)
0141:     pub affinity: u64,
0142:     /// Bu görev gerçek zamanlı mı? (FIFO veya RR politikası)
0143:     pub is_rt: bool,
0144: }
0145: 
0146: impl RtTaskInfo {
0147:     pub fn new(task_id: TaskId) -> Self {
0148:         Self {
0149:             task_id,
0150:             policy: SchedPolicy::Normal,
0151:             priority: 0,
0152:             time_slice: RR_DEFAULT_TIMESLICE,
0153:             total_timeslice: RR_DEFAULT_TIMESLICE,
0154:             affinity: 0xFFFFFFFFFFFFFFFF, // Tüm CPU'lar
0155:             is_rt: false,
0156:         }
0157:     }
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 07 (line 159)

```rust
0149:             task_id,
0150:             policy: SchedPolicy::Normal,
0151:             priority: 0,
0152:             time_slice: RR_DEFAULT_TIMESLICE,
0153:             total_timeslice: RR_DEFAULT_TIMESLICE,
0154:             affinity: 0xFFFFFFFFFFFFFFFF, // Tüm CPU'lar
0155:             is_rt: false,
0156:         }
0157:     }
0158: 
0159:     pub fn with_rt(task_id: TaskId, policy: SchedPolicy, priority: i32) -> Self {
0160:         let is_rt = policy == SchedPolicy::Fifo || policy == SchedPolicy::RoundRobin;
0161:         let time_slice = if policy == SchedPolicy::RoundRobin {
0162:             Self::calculate_timeslice(priority)
0163:         } else {
0164:             u64::MAX // FIFO: bloke veya yield edilene kadar çalışır
0165:         };
0166: 
0167:         Self {
0168:             task_id,
0169:             policy,
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 08 (line 180)

```rust
0170:             priority,
0171:             time_slice,
0172:             total_timeslice: time_slice,
0173:             affinity: 0xFFFFFFFFFFFFFFFF,
0174:             is_rt,
0175:         }
0176:     }
0177: 
0178:     /// Önceliğe göre zaman dilimini hesaplar.
0179:     /// Yüksek öncelik = daha uzun zaman dilimi
0180:     fn calculate_timeslice(priority: i32) -> u64 {
0181:         let normalized = (priority as f64 / RT_PRIO_MAX as f64).clamp(0.0, 1.0);
0182:         let slice =
0183:             RR_MIN_TIMESLICE as f64 + normalized * (RR_MAX_TIMESLICE - RR_MIN_TIMESLICE) as f64;
0184:         slice as u64
0185:     }
0186: 
0187:     /// Zaman dilimini sıfırlar (görev yeniden zamanlandığında çağrılır).
0188:     pub fn reset_timeslice(&mut self) {
0189:         self.time_slice = self.total_timeslice;
0190:     }
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 09 (line 188)

```rust
0178:     /// Önceliğe göre zaman dilimini hesaplar.
0179:     /// Yüksek öncelik = daha uzun zaman dilimi
0180:     fn calculate_timeslice(priority: i32) -> u64 {
0181:         let normalized = (priority as f64 / RT_PRIO_MAX as f64).clamp(0.0, 1.0);
0182:         let slice =
0183:             RR_MIN_TIMESLICE as f64 + normalized * (RR_MAX_TIMESLICE - RR_MIN_TIMESLICE) as f64;
0184:         slice as u64
0185:     }
0186: 
0187:     /// Zaman dilimini sıfırlar (görev yeniden zamanlandığında çağrılır).
0188:     pub fn reset_timeslice(&mut self) {
0189:         self.time_slice = self.total_timeslice;
0190:     }
0191: 
0192:     /// Zaman dilimini bir tick azaltır.
0193:     /// true döndürürse zaman dilimi doldu; yeniden zamanlama gerekli.
0194:     pub fn tick(&mut self) -> bool {
0195:         if self.policy == SchedPolicy::RoundRobin && self.time_slice > 0 {
0196:             self.time_slice = self.time_slice.saturating_sub(1);
0197:             return self.time_slice == 0;
0198:         }
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 10 (line 194)

```rust
0184:         slice as u64
0185:     }
0186: 
0187:     /// Zaman dilimini sıfırlar (görev yeniden zamanlandığında çağrılır).
0188:     pub fn reset_timeslice(&mut self) {
0189:         self.time_slice = self.total_timeslice;
0190:     }
0191: 
0192:     /// Zaman dilimini bir tick azaltır.
0193:     /// true döndürürse zaman dilimi doldu; yeniden zamanlama gerekli.
0194:     pub fn tick(&mut self) -> bool {
0195:         if self.policy == SchedPolicy::RoundRobin && self.time_slice > 0 {
0196:             self.time_slice = self.time_slice.saturating_sub(1);
0197:             return self.time_slice == 0;
0198:         }
0199:         false
0200:     }
0201: }
0202: 
0203: // ============================================================================
0204: // GERÇEK ZAMANLI ÇALIŞMA KUYRUĞU
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 11 (line 214)

```rust
0204: // GERÇEK ZAMANLI ÇALIŞMA KUYRUĞU
0205: // ============================================================================
0206: 
0207: /// Gerçek zamanlı çalışma kuyruğu (önceliğe göre sıralı)
0208: ///
0209: /// RT görevler öncelik kovalarına (1-99) kaydedilir.
0210: /// Yüksek öncelikli görevler her zaman düşük öncelikten önce çalışır.
0211: /// Aynı öncelik içinde:
0212: /// - SCHED_FIFO: İlk gelen ilk çalışır (FIFO sırası)
0213: /// - SCHED_RR: Zaman dilimleriyle döngüsel (round-robin)
0214: pub struct RtRunQueue {
0215:     /// Öncelik kovaları: öncelik → görev listesi
0216:     /// 99 en yüksek, 1 en düşük RT önceliğidir
0217:     queues: BTreeMap<i32, Vec<Box<Task>>>,
0218:     /// Görev ID → RT bilgi eşlemesi
0219:     task_info: BTreeMap<TaskId, RtTaskInfo>,
0220:     /// RT görev sayısı
0221:     rt_count: AtomicU64,
0222:     /// Çalışabilir görev bulunan en yüksek öncelik
0223:     highest_prio: AtomicU64,
0224:     /// RT kısıtlama: bant genişliği kontrolü (CPU zamanının en fazla %95'ini kullanabilir)
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 12 (line 231)

```rust
0221:     rt_count: AtomicU64,
0222:     /// Çalışabilir görev bulunan en yüksek öncelik
0223:     highest_prio: AtomicU64,
0224:     /// RT kısıtlama: bant genişliği kontrolü (CPU zamanının en fazla %95'ini kullanabilir)
0225:     rt_runtime: AtomicU64,
0226:     rt_period: AtomicU64,
0227:     rt_runtime_enabled: AtomicBool,
0228: }
0229: 
0230: impl RtRunQueue {
0231:     pub fn new() -> Self {
0232:         Self {
0233:             queues: BTreeMap::new(),
0234:             task_info: BTreeMap::new(),
0235:             rt_count: AtomicU64::new(0),
0236:             highest_prio: AtomicU64::new(0),
0237:             rt_runtime: AtomicU64::new(950_000_000), // 1s'nin %95'i
0238:             rt_period: AtomicU64::new(1_000_000_000), // 1s
0239:             rt_runtime_enabled: AtomicBool::new(true),
0240:         }
0241:     }
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 13 (line 244)

```rust
0234:             task_info: BTreeMap::new(),
0235:             rt_count: AtomicU64::new(0),
0236:             highest_prio: AtomicU64::new(0),
0237:             rt_runtime: AtomicU64::new(950_000_000), // 1s'nin %95'i
0238:             rt_period: AtomicU64::new(1_000_000_000), // 1s
0239:             rt_runtime_enabled: AtomicBool::new(true),
0240:         }
0241:     }
0242: 
0243:     /// RT çalışma kuyruğuna görev ekler.
0244:     pub fn enqueue(&mut self, task: Box<Task>) {
0245:         let task_id = task.hot.id;
0246:         let info = self
0247:             .task_info
0248:             .entry(task_id)
0249:             .or_insert_with(|| RtTaskInfo::new(task_id));
0250: 
0251:         let priority = info.priority;
0252:         let is_rt = info.is_rt;
0253: 
0254:         // Uygun öncelik kuyruğuna ekle
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 14 (line 269)

```rust
0259:         if is_rt && priority as u64 > self.highest_prio.load(Ordering::Relaxed) {
0260:             self.highest_prio.store(priority as u64, Ordering::Relaxed);
0261:         }
0262: 
0263:         if is_rt {
0264:             self.rt_count.fetch_add(1, Ordering::Relaxed);
0265:         }
0266:     }
0267: 
0268:     /// RT çalışma kuyruğundan görev çıkarır.
0269:     pub fn dequeue(&mut self, task_id: TaskId) -> Option<Box<Task>> {
0270:         let info = self.task_info.get(&task_id)?;
0271:         let priority = info.priority;
0272: 
0273:         if let Some(queue) = self.queues.get_mut(&priority) {
0274:             // Görevi bul ve kaldır
0275:             for i in 0..queue.len() {
0276:                 if queue[i].hot.id == task_id {
0277:                     let task = queue.remove(i);
0278:                     if info.is_rt {
0279:                         self.rt_count.fetch_sub(1, Ordering::Relaxed);
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 15 (line 295)

```rust
0285:                     }
0286:                     return Some(task);
0287:                 }
0288:             }
0289:         }
0290:         None
0291:     }
0292: 
0293:     /// Bir sonraki çalışacak görevi seçer.
0294:     /// En yüksek öncelikli RT görevi döndürür; RT görev yoksa None.
0295:     pub fn pick_next(&mut self) -> Option<Box<Task>> {
0296:         // Çalışabilir görev bulunan en yüksek önceliği bul
0297:         let highest = self.find_highest_prio();
0298:         if highest == 0 {
0299:             return None;
0300:         }
0301: 
0302:         if let Some(queue) = self.queues.get_mut(&highest) {
0303:             if !queue.is_empty() {
0304:                 // SCHED_RR: kuyruğu döndür (round-robin)
0305:                 // SCHED_FIFO: önden al (FIFO sırası)
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 16 (line 323)

```rust
0313:                     }
0314:                 }
0315: 
0316:                 return Some(task);
0317:             }
0318:         }
0319:         None
0320:     }
0321: 
0322:     /// Çalışabilir görev bulunan en yüksek önceliği bulur.
0323:     fn find_highest_prio(&self) -> i32 {
0324:         // BTreeMap sıralı iterasyon yapar; boş olmayan en yüksek anahtarı al
0325:         self.queues
0326:             .iter()
0327:             .rev()
0328:             .find(|(_, q)| !q.is_empty())
0329:             .map(|(p, _)| *p)
0330:             .unwrap_or(0)
0331:     }
0332: 
0333:     /// En yüksek öncelik izleme değerini günceller.
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 17 (line 334)

```rust
0324:         // BTreeMap sıralı iterasyon yapar; boş olmayan en yüksek anahtarı al
0325:         self.queues
0326:             .iter()
0327:             .rev()
0328:             .find(|(_, q)| !q.is_empty())
0329:             .map(|(p, _)| *p)
0330:             .unwrap_or(0)
0331:     }
0332: 
0333:     /// En yüksek öncelik izleme değerini günceller.
0334:     fn update_highest_prio(&mut self) {
0335:         let highest = self.find_highest_prio();
0336:         self.highest_prio.store(highest as u64, Ordering::Relaxed);
0337:     }
0338: 
0339:     /// RT görev sayısını döndürür.
0340:     pub fn rt_task_count(&self) -> u64 {
0341:         self.rt_count.load(Ordering::Relaxed)
0342:     }
0343: 
0344:     /// Çalışabilir RT görev var mı kontrol eder.
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 18 (line 340)

```rust
0330:             .unwrap_or(0)
0331:     }
0332: 
0333:     /// En yüksek öncelik izleme değerini günceller.
0334:     fn update_highest_prio(&mut self) {
0335:         let highest = self.find_highest_prio();
0336:         self.highest_prio.store(highest as u64, Ordering::Relaxed);
0337:     }
0338: 
0339:     /// RT görev sayısını döndürür.
0340:     pub fn rt_task_count(&self) -> u64 {
0341:         self.rt_count.load(Ordering::Relaxed)
0342:     }
0343: 
0344:     /// Çalışabilir RT görev var mı kontrol eder.
0345:     pub fn has_rt_tasks(&self) -> bool {
0346:         self.rt_count.load(Ordering::Relaxed) > 0
0347:     }
0348: 
0349:     /// Bir görev için zamanlama parametrelerini alır/ayarlar.
0350:     pub fn set_sched_param(&mut self, task_id: TaskId, policy: SchedPolicy, param: &RtSchedParam) {
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 19 (line 345)

```rust
0335:         let highest = self.find_highest_prio();
0336:         self.highest_prio.store(highest as u64, Ordering::Relaxed);
0337:     }
0338: 
0339:     /// RT görev sayısını döndürür.
0340:     pub fn rt_task_count(&self) -> u64 {
0341:         self.rt_count.load(Ordering::Relaxed)
0342:     }
0343: 
0344:     /// Çalışabilir RT görev var mı kontrol eder.
0345:     pub fn has_rt_tasks(&self) -> bool {
0346:         self.rt_count.load(Ordering::Relaxed) > 0
0347:     }
0348: 
0349:     /// Bir görev için zamanlama parametrelerini alır/ayarlar.
0350:     pub fn set_sched_param(&mut self, task_id: TaskId, policy: SchedPolicy, param: &RtSchedParam) {
0351:         let info = self
0352:             .task_info
0353:             .entry(task_id)
0354:             .or_insert_with(|| RtTaskInfo::new(task_id));
0355: 
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 20 (line 350)

```rust
0340:     pub fn rt_task_count(&self) -> u64 {
0341:         self.rt_count.load(Ordering::Relaxed)
0342:     }
0343: 
0344:     /// Çalışabilir RT görev var mı kontrol eder.
0345:     pub fn has_rt_tasks(&self) -> bool {
0346:         self.rt_count.load(Ordering::Relaxed) > 0
0347:     }
0348: 
0349:     /// Bir görev için zamanlama parametrelerini alır/ayarlar.
0350:     pub fn set_sched_param(&mut self, task_id: TaskId, policy: SchedPolicy, param: &RtSchedParam) {
0351:         let info = self
0352:             .task_info
0353:             .entry(task_id)
0354:             .or_insert_with(|| RtTaskInfo::new(task_id));
0355: 
0356:         let old_is_rt = info.is_rt;
0357: 
0358:         info.policy = policy;
0359:         info.priority = param.sched_priority.clamp(0, RT_PRIO_MAX);
0360:         info.is_rt = policy == SchedPolicy::Fifo || policy == SchedPolicy::RoundRobin;
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

---

## src/task/cfs.rs

- Satir sayisi: 481
- Derin kesit sayisi: 20

### Kesit 01 (line 64)

```rust
0054: /// nice=0 için referans ağırlık (Linux'ta aynı değer kullanılır)
0055: pub const CFS_NICE_0_WEIGHT: u64 = 1024;
0056: /// Yük ortalaması periyodu (PELT algoritması için)
0057: pub const CFS_LOAD_AVG_PERIOD: u64 = 32;
0058: /// PELT yarı ömrü — yük ortalamasının %50'ye düşmesi için gereken ms
0059: pub const CFS_PELT_HALF_LIFE: u64 = 32; // 32ms
0060: 
0061: /// nice değerinden ağırlık hesaplar.
0062: ///
0063: /// Her nice seviyesi ağırlığı yaklaşık %25 artırır veya azaltır.
0064: pub fn nice_to_weight(nice: i32) -> u64 {
0065:     let weight = CFS_NICE_0_WEIGHT as i64;
0066:     let delta = nice as i64;
0067: 
0068:     // Her nice seviyesi ağırlığı ~%25 oranında değiştirir
0069:     let factor = 1.25_f64.powi(delta.abs() as i32);
0070: 
0071:     if delta > 0 {
0072:         (weight as f64 / factor) as u64
0073:     } else {
0074:         (weight as f64 * factor) as u64
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 02 (line 82)

```rust
0072:         (weight as f64 / factor) as u64
0073:     } else {
0074:         (weight as f64 * factor) as u64
0075:     }
0076: }
0077: 
0078: /// Gerçek süreyi sanal çalışma zamanına (vruntime) dönüştürür.
0079: ///
0080: /// Formül: vruntime_delta = (delta * NICE_0_WEIGHT) / task_weight
0081: /// Ağır task'lar daha az vruntime biriktirerek daha sık seçilir.
0082: pub fn weight_to_vruntime(delta: u64, weight: u64) -> u64 {
0083:     if weight == 0 {
0084:         return delta;
0085:     }
0086:     (delta * CFS_NICE_0_WEIGHT) / weight
0087: }
0088: 
0089: // ============================================================================
0090: // CFS TASK (GÖREVİ)
0091: // ============================================================================
0092: 
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 03 (line 94)

```rust
0084:         return delta;
0085:     }
0086:     (delta * CFS_NICE_0_WEIGHT) / weight
0087: }
0088: 
0089: // ============================================================================
0090: // CFS TASK (GÖREVİ)
0091: // ============================================================================
0092: 
0093: #[derive(Clone, Debug)]
0094: pub struct CfsTask {
0095:     /// Görevin benzersiz kimlik numarası
0096:     pub task_id: u64,
0097:     /// nice değeri (-20 ile +19 arası; negatif = yüksek öncelik)
0098:     pub nice: AtomicI64,
0099:     /// Zamanlayıcı ağırlığı (nice değerinden türetilir)
0100:     pub weight: AtomicU64,
0101:     /// Sanal çalışma zamanı — CFS'in çekirdek değeri, ağaçta sıralama kriteri
0102:     pub vruntime: AtomicU64,
0103:     /// Toplam gerçek çalışma süresi (istatistik)
0104:     pub runtime: AtomicU64,
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 04 (line 122)

```rust
0112:     pub enqueue_time: AtomicU64,
0113:     /// PELT yük ortalaması (Per-Entity Load Tracking)
0114:     pub load_avg: AtomicU64,
0115:     /// PELT kullanım ortalaması
0116:     pub util_avg: AtomicU64,
0117:     /// Ayrıntılı istatistikler (bekleme süresi, migrasyon sayısı vb.)
0118:     pub stats: Mutex<CfsStats>,
0119: }
0120: 
0121: #[derive(Clone, Debug, Default)]
0122: pub struct CfsStats {
0123:     pub wait_start: u64,
0124:     pub wait_max: u64,
0125:     pub wait_count: u64,
0126:     pub wait_sum: u64,
0127:     pub iowait_count: u64,
0128:     pub iowait_sum: u64,
0129:     pub slices: u64,
0130:     pub migrations: u64,
0131: }
0132: 
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 05 (line 134)

```rust
0124:     pub wait_max: u64,
0125:     pub wait_count: u64,
0126:     pub wait_sum: u64,
0127:     pub iowait_count: u64,
0128:     pub iowait_sum: u64,
0129:     pub slices: u64,
0130:     pub migrations: u64,
0131: }
0132: 
0133: impl CfsTask {
0134:     pub fn new(task_id: u64, nice: i32) -> Self {
0135:         Self {
0136:             task_id,
0137:             nice: AtomicI64::new(nice as i64),
0138:             weight: AtomicU64::new(nice_to_weight(nice)),
0139:             vruntime: AtomicU64::new(0),
0140:             runtime: AtomicU64::new(0),
0141:             slice: AtomicU64::new(CFS_DEFAULT_SLICE),
0142:             running: AtomicBool::new(false),
0143:             on_rq: AtomicBool::new(false),
0144:             enqueue_time: AtomicU64::new(0),
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 06 (line 152)

```rust
0142:             running: AtomicBool::new(false),
0143:             on_rq: AtomicBool::new(false),
0144:             enqueue_time: AtomicU64::new(0),
0145:             load_avg: AtomicU64::new(0),
0146:             util_avg: AtomicU64::new(0),
0147:             stats: Mutex::new(CfsStats::default()),
0148:         }
0149:     }
0150: 
0151:     /// nice değerini günceller ve ağırlığı yeniden hesaplar.
0152:     pub fn set_nice(&self, nice: i32) {
0153:         self.nice.store(nice as i64, Ordering::SeqCst);
0154:         self.weight.store(nice_to_weight(nice), Ordering::SeqCst);
0155:     }
0156: 
0157:     /// Çalışma sonrası vruntime'ı günceller.
0158:     /// delta: gerçek çalışma süresi (nanosaniye)
0159:     pub fn update_vruntime(&self, delta: u64) {
0160:         let weight = self.weight.load(Ordering::Relaxed);
0161:         let vruntime_delta = weight_to_vruntime(delta, weight);
0162:         self.vruntime.fetch_add(vruntime_delta, Ordering::Relaxed);
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 07 (line 159)

```rust
0149:     }
0150: 
0151:     /// nice değerini günceller ve ağırlığı yeniden hesaplar.
0152:     pub fn set_nice(&self, nice: i32) {
0153:         self.nice.store(nice as i64, Ordering::SeqCst);
0154:         self.weight.store(nice_to_weight(nice), Ordering::SeqCst);
0155:     }
0156: 
0157:     /// Çalışma sonrası vruntime'ı günceller.
0158:     /// delta: gerçek çalışma süresi (nanosaniye)
0159:     pub fn update_vruntime(&self, delta: u64) {
0160:         let weight = self.weight.load(Ordering::Relaxed);
0161:         let vruntime_delta = weight_to_vruntime(delta, weight);
0162:         self.vruntime.fetch_add(vruntime_delta, Ordering::Relaxed);
0163:         self.runtime.fetch_add(delta, Ordering::Relaxed);
0164:     }
0165: 
0166:     /// Ağırlığa göre zaman dilimini hesaplar.
0167:     /// Yüksek ağırlıklı task'lar daha uzun dilim alır.
0168:     pub fn calc_slice(&self, total_weight: u64, nr_running: u64) -> u64 {
0169:         if nr_running == 0 {
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 08 (line 168)

```rust
0158:     /// delta: gerçek çalışma süresi (nanosaniye)
0159:     pub fn update_vruntime(&self, delta: u64) {
0160:         let weight = self.weight.load(Ordering::Relaxed);
0161:         let vruntime_delta = weight_to_vruntime(delta, weight);
0162:         self.vruntime.fetch_add(vruntime_delta, Ordering::Relaxed);
0163:         self.runtime.fetch_add(delta, Ordering::Relaxed);
0164:     }
0165: 
0166:     /// Ağırlığa göre zaman dilimini hesaplar.
0167:     /// Yüksek ağırlıklı task'lar daha uzun dilim alır.
0168:     pub fn calc_slice(&self, total_weight: u64, nr_running: u64) -> u64 {
0169:         if nr_running == 0 {
0170:             return CFS_DEFAULT_SLICE;
0171:         }
0172: 
0173:         let weight = self.weight.load(Ordering::Relaxed);
0174:         let slice = (weight * CFS_DEFAULT_SLICE * nr_running) / total_weight;
0175: 
0176:         slice.max(CFS_MIN_GRANULARITY)
0177:     }
0178: 
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 09 (line 181)

```rust
0171:         }
0172: 
0173:         let weight = self.weight.load(Ordering::Relaxed);
0174:         let slice = (weight * CFS_DEFAULT_SLICE * nr_running) / total_weight;
0175: 
0176:         slice.max(CFS_MIN_GRANULARITY)
0177:     }
0178: 
0179:     /// Task'ın çalışmaya uygun olup olmadığını kontrol eder.
0180:     /// min_vruntime'dan büyük vruntime'a sahip task'lar bekletilir.
0181:     pub fn is_eligible(&self, min_vruntime: u64) -> bool {
0182:         self.vruntime.load(Ordering::Relaxed) <= min_vruntime
0183:     }
0184: }
0185: 
0186: // ============================================================================
0187: // CFS ÇALIŞMA KUYRUĞU (RUN QUEUE)
0188: // ============================================================================
0189: //
0190: // CFS run queue'su kavramsal olarak bir Red-Black Tree'dir.
0191: // Burada BTreeMap ile simüle edilmiştir (key = vruntime).
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 10 (line 205)

```rust
0195: //   │  CfsRq (Çalışma Kuyruğu)                │
0196: //   │                                          │
0197: //   │  BTreeMap<vruntime, CfsTask>             │
0198: //   │  ┌────┬────┬────┬────┬────┐             │
0199: //   │  │ 10 │ 30 │ 50 │ 80 │100 │  <-- sol   │
0200: //   │  └────┴────┴────┴────┴────┘  en düşük  │
0201: //   │    ▲                                     │
0202: //   │  pick_next() burayı seçer                │
0203: //   └──────────────────────────────────────────┘
0204: 
0205: pub struct CfsRq {
0206:     /// vruntime'a göre sıralanmış task listesi (Red-Black Tree simülasyonu)
0207:     pub tasks: Mutex<BTreeMap<u64, Arc<CfsTask>>>, // vruntime -> task
0208:     /// Kuyrukta en küçük vruntime değeri — yeni task'lar buna göre ayarlanır
0209:     pub min_vruntime: AtomicU64,
0210:     /// Kuyruktaki tüm task'ların toplam ağırlığı (zaman dilimi hesabı için)
0211:     pub total_weight: AtomicU64,
0212:     /// Kuyruktaki çalışabilir task sayısı
0213:     pub nr_running: AtomicU32,
0214:     /// Şu an CPU'da çalışan task
0215:     pub curr: Mutex<Option<Arc<CfsTask>>>,
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 11 (line 225)

```rust
0215:     pub curr: Mutex<Option<Arc<CfsTask>>>,
0216:     /// PELT yük ortalaması (tüm kuyruk)
0217:     pub load_avg: AtomicU64,
0218:     /// PELT kullanım ortalaması (tüm kuyruk)
0219:     pub util_avg: AtomicU64,
0220:     /// Monoton artan mantıksal saat
0221:     pub clock: AtomicU64,
0222: }
0223: 
0224: impl CfsRq {
0225:     pub fn new() -> Self {
0226:         Self {
0227:             tasks: Mutex::new(BTreeMap::new()),
0228:             min_vruntime: AtomicU64::new(0),
0229:             total_weight: AtomicU64::new(0),
0230:             nr_running: AtomicU32::new(0),
0231:             curr: Mutex::new(None),
0232:             load_avg: AtomicU64::new(0),
0233:             util_avg: AtomicU64::new(0),
0234:             clock: AtomicU64::new(0),
0235:         }
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 12 (line 241)

```rust
0231:             curr: Mutex::new(None),
0232:             load_avg: AtomicU64::new(0),
0233:             util_avg: AtomicU64::new(0),
0234:             clock: AtomicU64::new(0),
0235:         }
0236:     }
0237: 
0238:     /// Task'ı çalışma kuyruğuna ekler.
0239:     /// Yeni veya uyuyan task'lar min_vruntime'a sıfırlanır; böylece
0240:     /// çok uzun süre uyuyan task'lar aniden geçmişe dönmez.
0241:     pub fn enqueue(&self, task: Arc<CfsTask>) {
0242:         let vruntime = task.vruntime.load(Ordering::Relaxed);
0243: 
0244:         // vruntime en az min_vruntime kadar olmalıdır (fairness koruması)
0245:         let min_vr = self.min_vruntime.load(Ordering::Relaxed);
0246:         let adjusted_vr = vruntime.max(min_vr);
0247: 
0248:         task.vruntime.store(adjusted_vr, Ordering::SeqCst);
0249:         task.on_rq.store(true, Ordering::SeqCst);
0250:         task.enqueue_time.store(self.clock.load(Ordering::Relaxed), Ordering::SeqCst);
0251: 
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 13 (line 258)

```rust
0248:         task.vruntime.store(adjusted_vr, Ordering::SeqCst);
0249:         task.on_rq.store(true, Ordering::SeqCst);
0250:         task.enqueue_time.store(self.clock.load(Ordering::Relaxed), Ordering::SeqCst);
0251: 
0252:         self.tasks.lock().insert(adjusted_vr, task.clone());
0253:         self.total_weight.fetch_add(task.weight.load(Ordering::Relaxed), Ordering::SeqCst);
0254:         self.nr_running.fetch_add(1, Ordering::SeqCst);
0255:     }
0256: 
0257:     /// Task'ı kuyruktan çıkarır (bloklanma veya sonlanma durumunda).
0258:     pub fn dequeue(&self, task: &CfsTask) {
0259:         let vruntime = task.vruntime.load(Ordering::Relaxed);
0260: 
0261:         self.tasks.lock().remove(&vruntime);
0262:         self.total_weight.fetch_sub(task.weight.load(Ordering::Relaxed), Ordering::SeqCst);
0263:         self.nr_running.fetch_sub(1, Ordering::SeqCst);
0264:         task.on_rq.store(false, Ordering::SeqCst);
0265:     }
0266: 
0267:     /// Bir sonraki çalıştırılacak task'ı seçer.
0268:     /// Red-Black Tree'nin en sol yaprağı = en küçük vruntime = en çok hak kazanan task.
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 14 (line 269)

```rust
0259:         let vruntime = task.vruntime.load(Ordering::Relaxed);
0260: 
0261:         self.tasks.lock().remove(&vruntime);
0262:         self.total_weight.fetch_sub(task.weight.load(Ordering::Relaxed), Ordering::SeqCst);
0263:         self.nr_running.fetch_sub(1, Ordering::SeqCst);
0264:         task.on_rq.store(false, Ordering::SeqCst);
0265:     }
0266: 
0267:     /// Bir sonraki çalıştırılacak task'ı seçer.
0268:     /// Red-Black Tree'nin en sol yaprağı = en küçük vruntime = en çok hak kazanan task.
0269:     pub fn pick_next(&self) -> Option<Arc<CfsTask>> {
0270:         let tasks = self.tasks.lock();
0271: 
0272:         // En sol düğüm (en düşük vruntime) — O(log n) but effectively O(1) cached
0273:         if let Some((&vruntime, task)) = tasks.iter().next() {
0274:             // min_vruntime'ı güncelle — kuyruk saatini ileri taşır
0275:             self.min_vruntime.store(vruntime, Ordering::SeqCst);
0276: 
0277:             task.running.store(true, Ordering::SeqCst);
0278:             *self.curr.lock() = Some(task.clone());
0279: 
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 15 (line 287)

```rust
0277:             task.running.store(true, Ordering::SeqCst);
0278:             *self.curr.lock() = Some(task.clone());
0279: 
0280:             return Some(task.clone());
0281:         }
0282: 
0283:         None
0284:     }
0285: 
0286:     /// Önceki task'ı geri kuyruğa alır (preemption veya yield sonrası).
0287:     pub fn put_prev(&self, task: &CfsTask) {
0288:         task.running.store(false, Ordering::SeqCst);
0289: 
0290:         if task.on_rq.load(Ordering::Relaxed) {
0291:             // Güncellenmiş vruntime ile yeniden ekle
0292:             let vruntime = task.vruntime.load(Ordering::Relaxed);
0293:             self.tasks.lock().insert(vruntime, Arc::new(task.clone()));
0294:         }
0295:     }
0296: 
0297:     /// Mantıksal saati günceller.
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 16 (line 298)

```rust
0288:         task.running.store(false, Ordering::SeqCst);
0289: 
0290:         if task.on_rq.load(Ordering::Relaxed) {
0291:             // Güncellenmiş vruntime ile yeniden ekle
0292:             let vruntime = task.vruntime.load(Ordering::Relaxed);
0293:             self.tasks.lock().insert(vruntime, Arc::new(task.clone()));
0294:         }
0295:     }
0296: 
0297:     /// Mantıksal saati günceller.
0298:     pub fn update_clock(&self, now: u64) {
0299:         self.clock.store(now, Ordering::SeqCst);
0300:     }
0301: 
0302:     /// PELT (Per-Entity Load Tracking) yük ortalamasını günceller.
0303:     /// Yük = ağırlık × delta_süre (exponential moving average ile düzeltilir).
0304:     pub fn update_load_avg(&self, task: &CfsTask, delta: u64) {
0305:         // Basitleştirilmiş PELT hesabı
0306:         let weight = task.weight.load(Ordering::Relaxed);
0307:         let contribution = weight * delta;
0308: 
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 17 (line 304)

```rust
0294:         }
0295:     }
0296: 
0297:     /// Mantıksal saati günceller.
0298:     pub fn update_clock(&self, now: u64) {
0299:         self.clock.store(now, Ordering::SeqCst);
0300:     }
0301: 
0302:     /// PELT (Per-Entity Load Tracking) yük ortalamasını günceller.
0303:     /// Yük = ağırlık × delta_süre (exponential moving average ile düzeltilir).
0304:     pub fn update_load_avg(&self, task: &CfsTask, delta: u64) {
0305:         // Basitleştirilmiş PELT hesabı
0306:         let weight = task.weight.load(Ordering::Relaxed);
0307:         let contribution = weight * delta;
0308: 
0309:         task.load_avg.fetch_add(contribution, Ordering::Relaxed);
0310:         self.load_avg.fetch_add(contribution, Ordering::Relaxed);
0311:     }
0312: 
0313:     /// Uyanan bir task'ın mevcut task'ı preempt edip edemeyeceğini kontrol eder.
0314:     /// CFS_WAKEUP_GRANULARITY'den fazla vruntime avantajı varsa preempt edilir.
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 18 (line 315)

```rust
0305:         // Basitleştirilmiş PELT hesabı
0306:         let weight = task.weight.load(Ordering::Relaxed);
0307:         let contribution = weight * delta;
0308: 
0309:         task.load_avg.fetch_add(contribution, Ordering::Relaxed);
0310:         self.load_avg.fetch_add(contribution, Ordering::Relaxed);
0311:     }
0312: 
0313:     /// Uyanan bir task'ın mevcut task'ı preempt edip edemeyeceğini kontrol eder.
0314:     /// CFS_WAKEUP_GRANULARITY'den fazla vruntime avantajı varsa preempt edilir.
0315:     pub fn check_preempt_wakeup(&self, task: &CfsTask) -> bool {
0316:         let curr = self.curr.lock();
0317:         if let Some(curr_task) = curr.as_ref() {
0318:             let curr_vr = curr_task.vruntime.load(Ordering::Relaxed);
0319:             let task_vr = task.vruntime.load(Ordering::Relaxed);
0320: 
0321:             // Yeni task çok daha düşük vruntime'a sahipse preempt et
0322:             if task_vr + CFS_WAKEUP_GRANULARITY < curr_vr {
0323:                 return true;
0324:             }
0325:         }
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 19 (line 334)

```rust
0324:             }
0325:         }
0326:         false
0327:     }
0328: }
0329: 
0330: // ============================================================================
0331: // CFS ZAMANLAYICISI
0332: // ============================================================================
0333: 
0334: pub struct CfsScheduler {
0335:     /// CPU başına çalışma kuyruğu (SMP desteği)
0336:     pub run_queues: Mutex<Vec<CfsRq>>,
0337:     /// Sistem CPU sayısı
0338:     pub nr_cpus: usize,
0339:     /// Zamanlayıcı aktif mi?
0340:     pub enabled: AtomicBool,
0341:     /// Tick aralığı (nanosaniye cinsinden)
0342:     pub tick_interval: u64,
0343:     /// Yük dengeleme aralığı (load balancer ne sıklıkla çalışır)
0344:     pub lb_interval: u64,
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 20 (line 348)

```rust
0338:     pub nr_cpus: usize,
0339:     /// Zamanlayıcı aktif mi?
0340:     pub enabled: AtomicBool,
0341:     /// Tick aralığı (nanosaniye cinsinden)
0342:     pub tick_interval: u64,
0343:     /// Yük dengeleme aralığı (load balancer ne sıklıkla çalışır)
0344:     pub lb_interval: u64,
0345: }
0346: 
0347: impl CfsScheduler {
0348:     pub fn new(nr_cpus: usize) -> Self {
0349:         let mut rqs = Vec::new();
0350:         for _ in 0..nr_cpus {
0351:             rqs.push(CfsRq::new());
0352:         }
0353: 
0354:         Self {
0355:             run_queues: Mutex::new(rqs),
0356:             nr_cpus,
0357:             enabled: AtomicBool::new(true),
0358:             tick_interval: 1_000_000, // 1ms
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

---

## src/task/eevdf.rs

- Satir sayisi: 183
- Derin kesit sayisi: 15

### Kesit 01 (line 17)

```rust
0007: //! sağlar.
0008: 
0009: use alloc::collections::BTreeMap;
0010: use alloc::sync::Arc;
0011: use alloc::vec::Vec;
0012: use core::cmp::Ordering as CmpOrdering;
0013: use core::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
0014: use spin::Mutex;
0015: 
0016: #[derive(Debug)]
0017: pub struct EevdfTask {
0018:     pub task_id: u64,
0019:     pub weight: u64,
0020:     pub vruntime: AtomicU64,
0021:     pub lag: AtomicI64,
0022:     pub slice_ns: AtomicU64,
0023:     pub eligible_vtime: AtomicU64,
0024:     pub virtual_deadline: AtomicU64,
0025:     pub on_rq: AtomicBool,
0026: }
0027: 
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 02 (line 29)

```rust
0019:     pub weight: u64,
0020:     pub vruntime: AtomicU64,
0021:     pub lag: AtomicI64,
0022:     pub slice_ns: AtomicU64,
0023:     pub eligible_vtime: AtomicU64,
0024:     pub virtual_deadline: AtomicU64,
0025:     pub on_rq: AtomicBool,
0026: }
0027: 
0028: impl EevdfTask {
0029:     pub fn new(task_id: u64, weight: u64, slice_ns: u64) -> Self {
0030:         let safe_weight = weight.max(1);
0031:         let safe_slice = slice_ns.max(1);
0032:         Self {
0033:             task_id,
0034:             weight: safe_weight,
0035:             vruntime: AtomicU64::new(0),
0036:             lag: AtomicI64::new(0),
0037:             slice_ns: AtomicU64::new(safe_slice),
0038:             eligible_vtime: AtomicU64::new(0),
0039:             virtual_deadline: AtomicU64::new(safe_slice),
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 03 (line 44)

```rust
0034:             weight: safe_weight,
0035:             vruntime: AtomicU64::new(0),
0036:             lag: AtomicI64::new(0),
0037:             slice_ns: AtomicU64::new(safe_slice),
0038:             eligible_vtime: AtomicU64::new(0),
0039:             virtual_deadline: AtomicU64::new(safe_slice),
0040:             on_rq: AtomicBool::new(false),
0041:         }
0042:     }
0043: 
0044:     pub fn update_runtime(&self, delta_ns: u64, rq_vtime: u64) {
0045:         let delta_v = delta_ns.saturating_mul(1024) / self.weight.max(1);
0046:         let vr = self.vruntime.fetch_add(delta_v, Ordering::SeqCst) + delta_v;
0047:         let lag = rq_vtime as i64 - vr as i64;
0048:         self.lag.store(lag, Ordering::SeqCst);
0049:         let slice = self.slice_ns.load(Ordering::Relaxed).max(1);
0050:         let eligible = if lag >= 0 { rq_vtime } else { vr };
0051:         self.eligible_vtime.store(eligible, Ordering::SeqCst);
0052:         self.virtual_deadline
0053:             .store(eligible.saturating_add(slice), Ordering::SeqCst);
0054:     }
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 04 (line 58)

```rust
0048:         self.lag.store(lag, Ordering::SeqCst);
0049:         let slice = self.slice_ns.load(Ordering::Relaxed).max(1);
0050:         let eligible = if lag >= 0 { rq_vtime } else { vr };
0051:         self.eligible_vtime.store(eligible, Ordering::SeqCst);
0052:         self.virtual_deadline
0053:             .store(eligible.saturating_add(slice), Ordering::SeqCst);
0054:     }
0055: }
0056: 
0057: #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
0058: struct DeadlineKey {
0059:     vd: u64,
0060:     task_id: u64,
0061: }
0062: 
0063: #[derive(Debug, Default, Clone)]
0064: pub struct EevdfStats {
0065:     pub tasks: usize,
0066:     pub vtime: u64,
0067:     pub min_deadline: u64,
0068: }
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 05 (line 64)

```rust
0054:     }
0055: }
0056: 
0057: #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
0058: struct DeadlineKey {
0059:     vd: u64,
0060:     task_id: u64,
0061: }
0062: 
0063: #[derive(Debug, Default, Clone)]
0064: pub struct EevdfStats {
0065:     pub tasks: usize,
0066:     pub vtime: u64,
0067:     pub min_deadline: u64,
0068: }
0069: 
0070: pub struct EevdfRunQueue {
0071:     vtime: AtomicU64,
0072:     tasks: Mutex<BTreeMap<u64, Arc<EevdfTask>>>,
0073:     by_deadline: Mutex<BTreeMap<DeadlineKey, Arc<EevdfTask>>>,
0074: }
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 06 (line 70)

```rust
0060:     task_id: u64,
0061: }
0062: 
0063: #[derive(Debug, Default, Clone)]
0064: pub struct EevdfStats {
0065:     pub tasks: usize,
0066:     pub vtime: u64,
0067:     pub min_deadline: u64,
0068: }
0069: 
0070: pub struct EevdfRunQueue {
0071:     vtime: AtomicU64,
0072:     tasks: Mutex<BTreeMap<u64, Arc<EevdfTask>>>,
0073:     by_deadline: Mutex<BTreeMap<DeadlineKey, Arc<EevdfTask>>>,
0074: }
0075: 
0076: impl EevdfRunQueue {
0077:     pub fn new() -> Self {
0078:         Self {
0079:             vtime: AtomicU64::new(0),
0080:             tasks: Mutex::new(BTreeMap::new()),
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 07 (line 77)

```rust
0067:     pub min_deadline: u64,
0068: }
0069: 
0070: pub struct EevdfRunQueue {
0071:     vtime: AtomicU64,
0072:     tasks: Mutex<BTreeMap<u64, Arc<EevdfTask>>>,
0073:     by_deadline: Mutex<BTreeMap<DeadlineKey, Arc<EevdfTask>>>,
0074: }
0075: 
0076: impl EevdfRunQueue {
0077:     pub fn new() -> Self {
0078:         Self {
0079:             vtime: AtomicU64::new(0),
0080:             tasks: Mutex::new(BTreeMap::new()),
0081:             by_deadline: Mutex::new(BTreeMap::new()),
0082:         }
0083:     }
0084: 
0085:     pub fn vtime(&self) -> u64 {
0086:         self.vtime.load(Ordering::Acquire)
0087:     }
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 08 (line 85)

```rust
0075: 
0076: impl EevdfRunQueue {
0077:     pub fn new() -> Self {
0078:         Self {
0079:             vtime: AtomicU64::new(0),
0080:             tasks: Mutex::new(BTreeMap::new()),
0081:             by_deadline: Mutex::new(BTreeMap::new()),
0082:         }
0083:     }
0084: 
0085:     pub fn vtime(&self) -> u64 {
0086:         self.vtime.load(Ordering::Acquire)
0087:     }
0088: 
0089:     pub fn enqueue(&self, task: Arc<EevdfTask>) {
0090:         let rq_vtime = self.vtime();
0091:         task.update_runtime(0, rq_vtime);
0092:         task.on_rq.store(true, Ordering::Release);
0093:         let key = DeadlineKey {
0094:             vd: task.virtual_deadline.load(Ordering::Acquire),
0095:             task_id: task.task_id,
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 09 (line 89)

```rust
0079:             vtime: AtomicU64::new(0),
0080:             tasks: Mutex::new(BTreeMap::new()),
0081:             by_deadline: Mutex::new(BTreeMap::new()),
0082:         }
0083:     }
0084: 
0085:     pub fn vtime(&self) -> u64 {
0086:         self.vtime.load(Ordering::Acquire)
0087:     }
0088: 
0089:     pub fn enqueue(&self, task: Arc<EevdfTask>) {
0090:         let rq_vtime = self.vtime();
0091:         task.update_runtime(0, rq_vtime);
0092:         task.on_rq.store(true, Ordering::Release);
0093:         let key = DeadlineKey {
0094:             vd: task.virtual_deadline.load(Ordering::Acquire),
0095:             task_id: task.task_id,
0096:         };
0097:         self.tasks.lock().insert(task.task_id, task.clone());
0098:         self.by_deadline.lock().insert(key, task);
0099:     }
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 10 (line 101)

```rust
0091:         task.update_runtime(0, rq_vtime);
0092:         task.on_rq.store(true, Ordering::Release);
0093:         let key = DeadlineKey {
0094:             vd: task.virtual_deadline.load(Ordering::Acquire),
0095:             task_id: task.task_id,
0096:         };
0097:         self.tasks.lock().insert(task.task_id, task.clone());
0098:         self.by_deadline.lock().insert(key, task);
0099:     }
0100: 
0101:     pub fn dequeue(&self, task_id: u64) -> Option<Arc<EevdfTask>> {
0102:         let task = self.tasks.lock().remove(&task_id)?;
0103:         let key = DeadlineKey {
0104:             vd: task.virtual_deadline.load(Ordering::Acquire),
0105:             task_id,
0106:         };
0107:         self.by_deadline.lock().remove(&key);
0108:         task.on_rq.store(false, Ordering::Release);
0109:         Some(task)
0110:     }
0111: 
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 11 (line 112)

```rust
0102:         let task = self.tasks.lock().remove(&task_id)?;
0103:         let key = DeadlineKey {
0104:             vd: task.virtual_deadline.load(Ordering::Acquire),
0105:             task_id,
0106:         };
0107:         self.by_deadline.lock().remove(&key);
0108:         task.on_rq.store(false, Ordering::Release);
0109:         Some(task)
0110:     }
0111: 
0112:     pub fn account_runtime(&self, task_id: u64, delta_ns: u64) {
0113:         let task = self.tasks.lock().get(&task_id).cloned();
0114:         let Some(task) = task else {
0115:             return;
0116:         };
0117: 
0118:         let old_key = DeadlineKey {
0119:             vd: task.virtual_deadline.load(Ordering::Acquire),
0120:             task_id,
0121:         };
0122:         self.by_deadline.lock().remove(&old_key);
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 12 (line 135)

```rust
0125:         self.vtime.store(next_vtime, Ordering::Release);
0126:         task.update_runtime(delta_ns, next_vtime);
0127: 
0128:         let new_key = DeadlineKey {
0129:             vd: task.virtual_deadline.load(Ordering::Acquire),
0130:             task_id,
0131:         };
0132:         self.by_deadline.lock().insert(new_key, task);
0133:     }
0134: 
0135:     pub fn pick_next(&self) -> Option<Arc<EevdfTask>> {
0136:         let rq_vtime = self.vtime();
0137:         for (_, task) in self.by_deadline.lock().iter() {
0138:             if task.eligible_vtime.load(Ordering::Acquire) <= rq_vtime {
0139:                 return Some(task.clone());
0140:             }
0141:         }
0142:         None
0143:     }
0144: 
0145:     pub fn should_preempt(&self, current_task_id: u64, wakee_task_id: u64) -> bool {
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 13 (line 145)

```rust
0135:     pub fn pick_next(&self) -> Option<Arc<EevdfTask>> {
0136:         let rq_vtime = self.vtime();
0137:         for (_, task) in self.by_deadline.lock().iter() {
0138:             if task.eligible_vtime.load(Ordering::Acquire) <= rq_vtime {
0139:                 return Some(task.clone());
0140:             }
0141:         }
0142:         None
0143:     }
0144: 
0145:     pub fn should_preempt(&self, current_task_id: u64, wakee_task_id: u64) -> bool {
0146:         let tasks = self.tasks.lock();
0147:         let current = match tasks.get(&current_task_id) {
0148:             Some(t) => t,
0149:             None => return false,
0150:         };
0151:         let wakee = match tasks.get(&wakee_task_id) {
0152:             Some(t) => t,
0153:             None => return false,
0154:         };
0155: 
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 14 (line 162)

```rust
0152:             Some(t) => t,
0153:             None => return false,
0154:         };
0155: 
0156:         let current_vd = current.virtual_deadline.load(Ordering::Acquire);
0157:         let wakee_vd = wakee.virtual_deadline.load(Ordering::Acquire);
0158:         let wakee_eligible = wakee.eligible_vtime.load(Ordering::Acquire) <= self.vtime();
0159:         wakee_eligible && wakee_vd < current_vd
0160:     }
0161: 
0162:     pub fn stats(&self) -> EevdfStats {
0163:         let by_deadline = self.by_deadline.lock();
0164:         let min_deadline = by_deadline
0165:             .iter()
0166:             .next()
0167:             .map(|(k, _)| k.vd)
0168:             .unwrap_or(0);
0169:         EevdfStats {
0170:             tasks: by_deadline.len(),
0171:             vtime: self.vtime(),
0172:             min_deadline,
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 15 (line 176)

```rust
0166:             .next()
0167:             .map(|(k, _)| k.vd)
0168:             .unwrap_or(0);
0169:         EevdfStats {
0170:             tasks: by_deadline.len(),
0171:             vtime: self.vtime(),
0172:             min_deadline,
0173:         }
0174:     }
0175: 
0176:     pub fn ordered_task_ids(&self) -> Vec<u64> {
0177:         self.by_deadline
0178:             .lock()
0179:             .iter()
0180:             .map(|(_, task)| task.task_id)
0181:             .collect()
0182:     }
0183: }
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

---

## src/task/deadline.rs

- Satir sayisi: 464
- Derin kesit sayisi: 20

### Kesit 01 (line 83)

```rust
0073: // ============================================================================
0074: // DEADLINE TASK (GÖREVİ)
0075: // ============================================================================
0076: //
0077: // Bir SCHED_DEADLINE task'ının üç temel parametresi vardır:
0078: //   runtime  (C): Her periyotta kullanabileceği maksimum CPU süresi
0079: //   deadline (D): Göreli son tarih (periyot başına göre)
0080: //   period   (T): Yineleme periyodu (en az D kadar olmalıdır)
0081: 
0082: #[derive(Clone, Debug)]
0083: pub struct DeadlineTask {
0084:     /// Görevin benzersiz kimlik numarası
0085:     pub task_id: u64,
0086:     /// CPU bütçesi — her periyotta başlangıç değeri (nanosaniye)
0087:     pub runtime: AtomicU64,
0088:     /// Kalan bütçe — her tick'te azalır, 0 olunca task throttle edilir
0089:     pub remaining_runtime: AtomicU64,
0090:     /// Periyot uzunluğu (nanosaniye)
0091:     pub period: u64,
0092:     /// Göreli son tarih (nanosaniye, periyot başlangıcından itibaren)
0093:     pub deadline: u64,
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 02 (line 109)

```rust
0099:     pub active: AtomicBool,
0100:     /// Bütçe tükendi mi? (throttled = CPU'ya erişim engellendi)
0101:     pub throttled: AtomicBool,
0102:     /// Zamanlama bayrakları
0103:     pub flags: u64,
0104:     /// İstatistikler (gecikme sayısı, toplam çalışma süresi vb.)
0105:     pub stats: Mutex<DlStats>,
0106: }
0107: 
0108: #[derive(Clone, Debug, Default)]
0109: pub struct DlStats {
0110:     pub migrations: u64,
0111:     pub throttled_time: u64,
0112:     pub runtime_time: u64,
0113:     pub deadline_misses: u64,
0114: }
0115: 
0116: impl DeadlineTask {
0117:     pub fn new(task_id: u64, runtime: u64, period: u64, deadline: u64, flags: u64) -> Self {
0118:         let now = crate::task::scheduler::get_ticks();
0119: 
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 03 (line 117)

```rust
0107: 
0108: #[derive(Clone, Debug, Default)]
0109: pub struct DlStats {
0110:     pub migrations: u64,
0111:     pub throttled_time: u64,
0112:     pub runtime_time: u64,
0113:     pub deadline_misses: u64,
0114: }
0115: 
0116: impl DeadlineTask {
0117:     pub fn new(task_id: u64, runtime: u64, period: u64, deadline: u64, flags: u64) -> Self {
0118:         let now = crate::task::scheduler::get_ticks();
0119: 
0120:         Self {
0121:             task_id,
0122:             runtime: AtomicU64::new(runtime),
0123:             remaining_runtime: AtomicU64::new(runtime),
0124:             period,
0125:             deadline,
0126:             abs_deadline: AtomicU64::new(now + deadline),
0127:             next_replenish: AtomicU64::new(now + period),
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 04 (line 137)

```rust
0127:             next_replenish: AtomicU64::new(now + period),
0128:             active: AtomicBool::new(true),
0129:             throttled: AtomicBool::new(false),
0130:             flags,
0131:             stats: Mutex::new(DlStats::default()),
0132:         }
0133:     }
0134: 
0135:     /// Son tarihin geçip geçmediğini kontrol eder.
0136:     /// Eğer geçtiyse "deadline miss" olarak kaydedilir.
0137:     pub fn deadline_passed(&self) -> bool {
0138:         let now = crate::task::scheduler::get_ticks();
0139:         now > self.abs_deadline.load(Ordering::Relaxed)
0140:     }
0141: 
0142:     /// Bütçenin tükenip tükenmediğini kontrol eder.
0143:     pub fn runtime_exhausted(&self) -> bool {
0144:         self.remaining_runtime.load(Ordering::Relaxed) == 0
0145:     }
0146: 
0147:     /// Çalışma süresini bütçeden düşer.
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 05 (line 143)

```rust
0133:     }
0134: 
0135:     /// Son tarihin geçip geçmediğini kontrol eder.
0136:     /// Eğer geçtiyse "deadline miss" olarak kaydedilir.
0137:     pub fn deadline_passed(&self) -> bool {
0138:         let now = crate::task::scheduler::get_ticks();
0139:         now > self.abs_deadline.load(Ordering::Relaxed)
0140:     }
0141: 
0142:     /// Bütçenin tükenip tükenmediğini kontrol eder.
0143:     pub fn runtime_exhausted(&self) -> bool {
0144:         self.remaining_runtime.load(Ordering::Relaxed) == 0
0145:     }
0146: 
0147:     /// Çalışma süresini bütçeden düşer.
0148:     /// Bütçe sıfıra ulaşırsa task throttle edilir.
0149:     pub fn consume_runtime(&self, ns: u64) {
0150:         let remaining = self.remaining_runtime.load(Ordering::Relaxed);
0151:         let new_remaining = remaining.saturating_sub(ns);
0152:         self.remaining_runtime.store(new_remaining, Ordering::Relaxed);
0153: 
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 06 (line 149)

```rust
0139:         now > self.abs_deadline.load(Ordering::Relaxed)
0140:     }
0141: 
0142:     /// Bütçenin tükenip tükenmediğini kontrol eder.
0143:     pub fn runtime_exhausted(&self) -> bool {
0144:         self.remaining_runtime.load(Ordering::Relaxed) == 0
0145:     }
0146: 
0147:     /// Çalışma süresini bütçeden düşer.
0148:     /// Bütçe sıfıra ulaşırsa task throttle edilir.
0149:     pub fn consume_runtime(&self, ns: u64) {
0150:         let remaining = self.remaining_runtime.load(Ordering::Relaxed);
0151:         let new_remaining = remaining.saturating_sub(ns);
0152:         self.remaining_runtime.store(new_remaining, Ordering::Relaxed);
0153: 
0154:         if new_remaining == 0 {
0155:             self.throttled.store(true, Ordering::SeqCst);
0156:         }
0157:     }
0158: 
0159:     /// Yeni periyot başında bütçeyi yeniler.
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 07 (line 161)

```rust
0151:         let new_remaining = remaining.saturating_sub(ns);
0152:         self.remaining_runtime.store(new_remaining, Ordering::Relaxed);
0153: 
0154:         if new_remaining == 0 {
0155:             self.throttled.store(true, Ordering::SeqCst);
0156:         }
0157:     }
0158: 
0159:     /// Yeni periyot başında bütçeyi yeniler.
0160:     /// CBS (Constant Bandwidth Server) davranışı burada uygulanır.
0161:     pub fn replenish(&self) {
0162:         let now = crate::task::scheduler::get_ticks();
0163:         let runtime = self.runtime.load(Ordering::Relaxed);
0164: 
0165:         // Yeni mutlak son tarihi hesapla
0166:         let new_deadline = now + self.deadline;
0167:         self.abs_deadline.store(new_deadline, Ordering::SeqCst);
0168: 
0169:         // Bütçeyi tam olarak yenile
0170:         self.remaining_runtime.store(runtime, Ordering::SeqCst);
0171: 
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 08 (line 184)

```rust
0174: 
0175:         // Throttle bayrağını kaldır — görev çalışabilir
0176:         self.throttled.store(false, Ordering::SeqCst);
0177: 
0178:         crate::serial_println!("[DL] Task {} bütçe yenilendi, son_tarih={}",
0179:             self.task_id, new_deadline);
0180:     }
0181: 
0182:     /// Gevşeklik (laxity) hesaplar: son_tarih - şimdi - kalan_bütçe.
0183:     /// Negatif laxity = son tarihi kaçırma riski!
0184:     pub fn laxity(&self) -> i64 {
0185:         let now = crate::task::scheduler::get_ticks();
0186:         let deadline = self.abs_deadline.load(Ordering::Relaxed) as i64;
0187:         let remaining = self.remaining_runtime.load(Ordering::Relaxed) as i64;
0188: 
0189:         deadline - now as i64 - remaining
0190:     }
0191: 
0192:     /// EDF sıralaması için iki task'ın son tarihlerini karşılaştırır.
0193:     pub fn compare_deadline(&self, other: &DeadlineTask) -> core::cmp::Ordering {
0194:         self.abs_deadline.load(Ordering::Relaxed)
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 09 (line 193)

```rust
0183:     /// Negatif laxity = son tarihi kaçırma riski!
0184:     pub fn laxity(&self) -> i64 {
0185:         let now = crate::task::scheduler::get_ticks();
0186:         let deadline = self.abs_deadline.load(Ordering::Relaxed) as i64;
0187:         let remaining = self.remaining_runtime.load(Ordering::Relaxed) as i64;
0188: 
0189:         deadline - now as i64 - remaining
0190:     }
0191: 
0192:     /// EDF sıralaması için iki task'ın son tarihlerini karşılaştırır.
0193:     pub fn compare_deadline(&self, other: &DeadlineTask) -> core::cmp::Ordering {
0194:         self.abs_deadline.load(Ordering::Relaxed)
0195:             .cmp(&other.abs_deadline.load(Ordering::Relaxed))
0196:     }
0197: }
0198: 
0199: // ============================================================================
0200: // DEADLINE ÇALIŞMA KUYRUĞU (RUN QUEUE)
0201: // ============================================================================
0202: //
0203: // EDF politikasında kuyruk her zaman son tarihe göre sıralıdır.
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 10 (line 213)

```rust
0203: // EDF politikasında kuyruk her zaman son tarihe göre sıralıdır.
0204: // En sol düğüm = en yakın son tarih = bir sonraki çalışacak task.
0205: //
0206: //   BTreeMap<abs_deadline, DeadlineTask>
0207: //   ┌──────┬──────┬──────┬──────┐
0208: //   │ t=10 │ t=15 │ t=20 │ t=30 │
0209: //   └──────┴──────┴──────┴──────┘
0210: //      ▲
0211: //   pick_next() burayı seçer
0212: 
0213: pub struct DeadlineRq {
0214:     /// Son tarihe göre sıralanmış görev listesi
0215:     pub tasks: Mutex<BTreeMap<u64, Arc<DeadlineTask>>>, // son_tarih -> görev
0216:     /// Şu anda CPU'da çalışan görev
0217:     pub running: Mutex<Option<Arc<DeadlineTask>>>,
0218:     /// Toplam bant genişliği kullanımı (U = Σ C_i / T_i * 10000)
0219:     pub total_bw: AtomicU64,
0220:     /// Maksimum izin verilen bant genişliği (10000 = %100)
0221:     pub max_bw: u64, // 10000 = %100
0222: }
0223: 
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 11 (line 225)

```rust
0215:     pub tasks: Mutex<BTreeMap<u64, Arc<DeadlineTask>>>, // son_tarih -> görev
0216:     /// Şu anda CPU'da çalışan görev
0217:     pub running: Mutex<Option<Arc<DeadlineTask>>>,
0218:     /// Toplam bant genişliği kullanımı (U = Σ C_i / T_i * 10000)
0219:     pub total_bw: AtomicU64,
0220:     /// Maksimum izin verilen bant genişliği (10000 = %100)
0221:     pub max_bw: u64, // 10000 = %100
0222: }
0223: 
0224: impl DeadlineRq {
0225:     pub fn new() -> Self {
0226:         Self {
0227:             tasks: Mutex::new(BTreeMap::new()),
0228:             running: Mutex::new(None),
0229:             total_bw: AtomicU64::new(0),
0230:             max_bw: 10000, // %100
0231:         }
0232:     }
0233: 
0234:     /// Görevi çalışma kuyruğuna ekler.
0235:     /// Bant genişliği kontrolü yapar: toplam U <= max_bw olmalı.
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 12 (line 236)

```rust
0226:         Self {
0227:             tasks: Mutex::new(BTreeMap::new()),
0228:             running: Mutex::new(None),
0229:             total_bw: AtomicU64::new(0),
0230:             max_bw: 10000, // %100
0231:         }
0232:     }
0233: 
0234:     /// Görevi çalışma kuyruğuna ekler.
0235:     /// Bant genişliği kontrolü yapar: toplam U <= max_bw olmalı.
0236:     pub fn enqueue(&self, task: Arc<DeadlineTask>) -> Result<(), DlError> {
0237:         // Kabul testi: yeni görevin bant genişliğini kontrol et
0238:         let task_bw = self.compute_bandwidth(&task);
0239:         let current_bw = self.total_bw.load(Ordering::Relaxed);
0240: 
0241:         if current_bw + task_bw > self.max_bw {
0242:             return Err(DlError::BandwidthExceeded);
0243:         }
0244: 
0245:         self.total_bw.fetch_add(task_bw, Ordering::Relaxed);
0246: 
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 13 (line 254)

```rust
0244: 
0245:         self.total_bw.fetch_add(task_bw, Ordering::Relaxed);
0246: 
0247:         let deadline = task.abs_deadline.load(Ordering::Relaxed);
0248:         self.tasks.lock().insert(deadline, task);
0249: 
0250:         Ok(())
0251:     }
0252: 
0253:     /// Görevi kuyruktan çıkarır ve bant genişliğini serbest bırakır.
0254:     pub fn dequeue(&self, task: &DeadlineTask) {
0255:         let deadline = task.abs_deadline.load(Ordering::Relaxed);
0256:         self.tasks.lock().remove(&deadline);
0257: 
0258:         let task_bw = self.compute_bandwidth(task);
0259:         self.total_bw.fetch_sub(task_bw, Ordering::Relaxed);
0260:     }
0261: 
0262:     /// EDF politikasına göre bir sonraki görevi seçer.
0263:     /// Throttle edilmemiş, aktif görevler arasında en yakın son tarihe sahip olanı döndürür.
0264:     pub fn pick_next(&self) -> Option<Arc<DeadlineTask>> {
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 14 (line 264)

```rust
0254:     pub fn dequeue(&self, task: &DeadlineTask) {
0255:         let deadline = task.abs_deadline.load(Ordering::Relaxed);
0256:         self.tasks.lock().remove(&deadline);
0257: 
0258:         let task_bw = self.compute_bandwidth(task);
0259:         self.total_bw.fetch_sub(task_bw, Ordering::Relaxed);
0260:     }
0261: 
0262:     /// EDF politikasına göre bir sonraki görevi seçer.
0263:     /// Throttle edilmemiş, aktif görevler arasında en yakın son tarihe sahip olanı döndürür.
0264:     pub fn pick_next(&self) -> Option<Arc<DeadlineTask>> {
0265:         let tasks = self.tasks.lock();
0266: 
0267:         // Son tarihe göre en yakın, throttle edilmemiş görevi bul
0268:         for task in tasks.values() {
0269:             if !task.throttled.load(Ordering::Relaxed) &&
0270:                task.active.load(Ordering::Relaxed) {
0271:                 return Some(task.clone());
0272:             }
0273:         }
0274: 
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 15 (line 280)

```rust
0270:                task.active.load(Ordering::Relaxed) {
0271:                 return Some(task.clone());
0272:             }
0273:         }
0274: 
0275:         None
0276:     }
0277: 
0278:     /// Bant genişliğini hesaplar (yüzde * 100 cinsinden).
0279:     /// U_i = (runtime / period) * 10000
0280:     fn compute_bandwidth(&self, task: &DeadlineTask) -> u64 {
0281:         let runtime = task.runtime.load(Ordering::Relaxed);
0282:         let period = task.period;
0283: 
0284:         if period == 0 {
0285:             return 0;
0286:         }
0287: 
0288:         // bant_genisligi = (runtime / period) * 10000
0289:         (runtime * 10000) / period
0290:     }
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 16 (line 293)

```rust
0283: 
0284:         if period == 0 {
0285:             return 0;
0286:         }
0287: 
0288:         // bant_genisligi = (runtime / period) * 10000
0289:         (runtime * 10000) / period
0290:     }
0291: 
0292:     /// Periyot sona eren görevlerin bütçelerini yeniler.
0293:     pub fn check_replenishments(&self) {
0294:         let now = crate::task::scheduler::get_ticks();
0295: 
0296:         for task in self.tasks.lock().values() {
0297:             if task.next_replenish.load(Ordering::Relaxed) <= now {
0298:                 task.replenish();
0299:             }
0300:         }
0301:     }
0302: 
0303:     /// Son tarihi geçmiş görevleri tespit eder ve istatistiğe kaydeder.
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 17 (line 304)

```rust
0294:         let now = crate::task::scheduler::get_ticks();
0295: 
0296:         for task in self.tasks.lock().values() {
0297:             if task.next_replenish.load(Ordering::Relaxed) <= now {
0298:                 task.replenish();
0299:             }
0300:         }
0301:     }
0302: 
0303:     /// Son tarihi geçmiş görevleri tespit eder ve istatistiğe kaydeder.
0304:     pub fn check_deadline_misses(&self) {
0305:         for task in self.tasks.lock().values() {
0306:             if task.deadline_passed() && !task.throttled.load(Ordering::Relaxed) {
0307:                 let mut stats = task.stats.lock();
0308:                 stats.deadline_misses += 1;
0309: 
0310:                 crate::serial_println!(
0311:                     "[DL] Görev {} son tarihi kaçırdı!",
0312:                     task.task_id
0313:                 );
0314:             }
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 18 (line 323)

```rust
0313:                 );
0314:             }
0315:         }
0316:     }
0317: }
0318: 
0319: // ============================================================================
0320: // DEADLINE ZAMANLAYICISI
0321: // ============================================================================
0322: 
0323: pub struct DeadlineScheduler {
0324:     /// CPU başına çalışma kuyruğu (SMP desteği)
0325:     pub run_queues: Mutex<Vec<DeadlineRq>>,
0326:     /// Sistem CPU sayısı
0327:     pub nr_cpus: usize,
0328:     /// Zamanlayıcı aktif mi?
0329:     pub enabled: AtomicBool,
0330:     /// Tick aralığı (nanosaniye cinsinden)
0331:     pub tick_interval: u64,
0332: }
0333: 
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 19 (line 335)

```rust
0325:     pub run_queues: Mutex<Vec<DeadlineRq>>,
0326:     /// Sistem CPU sayısı
0327:     pub nr_cpus: usize,
0328:     /// Zamanlayıcı aktif mi?
0329:     pub enabled: AtomicBool,
0330:     /// Tick aralığı (nanosaniye cinsinden)
0331:     pub tick_interval: u64,
0332: }
0333: 
0334: impl DeadlineScheduler {
0335:     pub fn new(nr_cpus: usize) -> Self {
0336:         let mut rqs = Vec::new();
0337:         for _ in 0..nr_cpus {
0338:             rqs.push(DeadlineRq::new());
0339:         }
0340: 
0341:         Self {
0342:             run_queues: Mutex::new(rqs),
0343:             nr_cpus,
0344:             enabled: AtomicBool::new(true),
0345:             tick_interval: 1_000_000, // 1ms
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 20 (line 351)

```rust
0341:         Self {
0342:             run_queues: Mutex::new(rqs),
0343:             nr_cpus,
0344:             enabled: AtomicBool::new(true),
0345:             tick_interval: 1_000_000, // 1ms
0346:         }
0347:     }
0348: 
0349:     /// Bir sonraki çalışacak görevi seçer.
0350:     /// Önce bütçe yenilemelerini kontrol eder, sonra EDF seçimi yapar.
0351:     pub fn schedule(&self, cpu: usize) -> Option<Arc<DeadlineTask>> {
0352:         let rqs = self.run_queues.lock();
0353:         if let Some(rq) = rqs.get(cpu) {
0354:             rq.check_replenishments();
0355:             rq.pick_next()
0356:         } else {
0357:             None
0358:         }
0359:     }
0360: 
0361:     /// Yeni bir SCHED_DEADLINE görevi ekler.
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

---

## src/task/deque.rs

- Satir sayisi: 201
- Derin kesit sayisi: 8

### Kesit 01 (line 46)

```rust
0036: use core::sync::atomic::{AtomicIsize, AtomicPtr, Ordering};
0037: 
0038: // Sabit boyutlu Chase-Lev Deque uygulaması.
0039: // Gerçek üretim sistemlerinde buffer yeniden boyutlandırma gerekir,
0040: // ancak şimdilik sabit kapasiteli sürüm kullanılıyor.
0041: // Kapasite: 4096 görev/CPU (Tier 1 OS standardı)
0042: const DEQUE_SIZE: usize = 4096;
0043: 
0044: /// Yerel (local) işlemci tarafından kullanılan Worker ucu.
0045: /// Sadece sahibi olan iş parçacığı güvenle push/pop yapabilir.
0046: pub struct Worker<T> {
0047:     inner: Arc<Inner<T>>,
0048: }
0049: 
0050: /// Uzak (remote) işlemcilerin iş çalmak için kullandığı Stealer ucu.
0051: /// Birden fazla Stealer aynı anda çalışabilir (lock-free CAS ile).
0052: pub struct Stealer<T> {
0053:     inner: Arc<Inner<T>>,
0054: }
0055: 
0056: // Worker ve Stealer'ın paylaştığı iç yapı
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 02 (line 52)

```rust
0042: const DEQUE_SIZE: usize = 4096;
0043: 
0044: /// Yerel (local) işlemci tarafından kullanılan Worker ucu.
0045: /// Sadece sahibi olan iş parçacığı güvenle push/pop yapabilir.
0046: pub struct Worker<T> {
0047:     inner: Arc<Inner<T>>,
0048: }
0049: 
0050: /// Uzak (remote) işlemcilerin iş çalmak için kullandığı Stealer ucu.
0051: /// Birden fazla Stealer aynı anda çalışabilir (lock-free CAS ile).
0052: pub struct Stealer<T> {
0053:     inner: Arc<Inner<T>>,
0054: }
0055: 
0056: // Worker ve Stealer'ın paylaştığı iç yapı
0057: struct Inner<T> {
0058:     buffer: [AtomicPtr<T>; DEQUE_SIZE],
0059:     top: AtomicIsize,    // Stealer'lar buradan çalar (steal) — baştan okur
0060:     bottom: AtomicIsize, // Worker buradan ekler/alır (push/pop) — sondan okur
0061: }
0062: 
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 03 (line 57)

```rust
0047:     inner: Arc<Inner<T>>,
0048: }
0049: 
0050: /// Uzak (remote) işlemcilerin iş çalmak için kullandığı Stealer ucu.
0051: /// Birden fazla Stealer aynı anda çalışabilir (lock-free CAS ile).
0052: pub struct Stealer<T> {
0053:     inner: Arc<Inner<T>>,
0054: }
0055: 
0056: // Worker ve Stealer'ın paylaştığı iç yapı
0057: struct Inner<T> {
0058:     buffer: [AtomicPtr<T>; DEQUE_SIZE],
0059:     top: AtomicIsize,    // Stealer'lar buradan çalar (steal) — baştan okur
0060:     bottom: AtomicIsize, // Worker buradan ekler/alır (push/pop) — sondan okur
0061: }
0062: 
0063: // Inner yapısının farklı thread'lere güvenle gönderilmesine izin ver
0064: unsafe impl<T: Send> Send for Inner<T> {}
0065: unsafe impl<T: Send> Sync for Inner<T> {}
0066: 
0067: impl<T> Worker<T> {
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 04 (line 70)

```rust
0060:     bottom: AtomicIsize, // Worker buradan ekler/alır (push/pop) — sondan okur
0061: }
0062: 
0063: // Inner yapısının farklı thread'lere güvenle gönderilmesine izin ver
0064: unsafe impl<T: Send> Send for Inner<T> {}
0065: unsafe impl<T: Send> Sync for Inner<T> {}
0066: 
0067: impl<T> Worker<T> {
0068:     /// Yeni bir Worker/Stealer çifti oluşturur.
0069:     /// İkisi de aynı iç tamponu paylaşır (Arc ile referans sayılı).
0070:     pub fn new() -> (Worker<T>, Stealer<T>) {
0071:         // Tamponu sıfır-başlatılmış null pointer'larla hazırla.
0072:         // AtomicPtr null pointer (0) ile başlatılabilir.
0073:         // x86_64 mimarisinde null pointer her zaman 0'dır, bu yüzden zeroed güvenlidir.
0074:         let mut buffer: [AtomicPtr<T>; DEQUE_SIZE] =
0075:             unsafe { core::mem::MaybeUninit::zeroed().assume_init() };
0076: 
0077:         let inner = Arc::new(Inner {
0078:             buffer,
0079:             top: AtomicIsize::new(0),
0080:             bottom: AtomicIsize::new(0),
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 05 (line 93)

```rust
0083:         (
0084:             Worker {
0085:                 inner: inner.clone(),
0086:             },
0087:             Stealer { inner },
0088:         )
0089:     }
0090: 
0091:     /// Kuyruğun sonuna (bottom) yeni bir görev ekler.
0092:     /// Sadece sahibi Worker tarafından çağrılabilir (tek üretici).
0093:     pub fn push(&self, task: Box<T>) {
0094:         let b = self.inner.bottom.load(Ordering::Relaxed);
0095:         let t = self.inner.top.load(Ordering::Acquire);
0096: 
0097:         if (b.wrapping_sub(t)) as usize >= DEQUE_SIZE {
0098:             // Tampon doldu! Daha fazla görev kabul edilemiyor.
0099:             panic!("Worker deque full! Cannot scale beyond 4096 tasks per CPU without resizing.");
0100:         }
0101: 
0102:         let task_ptr = Box::into_raw(task);
0103:         let idx = (b as usize) % DEQUE_SIZE;
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 06 (line 118)

```rust
0108:         self.inner
0109:             .bottom
0110:             .store(b.wrapping_add(1), Ordering::Relaxed);
0111:     }
0112: 
0113:     /// Kuyruğun sonundan (bottom) bir görev alır — LIFO davranışı.
0114:     /// Sadece sahibi Worker tarafından çağrılabilir.
0115:     ///
0116:     /// Son eleman için Stealer ile yarış durumu oluşabilir;
0117:     /// bu durumda CAS (Compare-And-Swap) ile çözülür.
0118:     pub fn pop(&self) -> Option<Box<T>> {
0119:         let b = self.inner.bottom.load(Ordering::Relaxed).wrapping_sub(1);
0120:         self.inner.bottom.store(b, Ordering::Relaxed);
0121:         core::sync::atomic::fence(Ordering::SeqCst);
0122: 
0123:         let t = self.inner.top.load(Ordering::Relaxed);
0124: 
0125:         if t <= b {
0126:             // Kuyruk boş değil — normal durum
0127:             let idx = (b as usize) % DEQUE_SIZE;
0128:             let task_ptr = self.inner.buffer[idx].load(Ordering::Relaxed);
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 07 (line 163)

```rust
0153:         } else {
0154:             // Kuyruk boştu — bottom'u geri al
0155:             self.inner
0156:                 .bottom
0157:                 .store(b.wrapping_add(1), Ordering::Relaxed);
0158:             return None;
0159:         }
0160:     }
0161: 
0162:     /// Kuyruktaki mevcut görev sayısını döndürür.
0163:     pub fn len(&self) -> usize {
0164:         let b = self.inner.bottom.load(Ordering::Relaxed);
0165:         let t = self.inner.top.load(Ordering::Relaxed);
0166:         if b < t {
0167:             0
0168:         } else {
0169:             (b - t) as usize
0170:         }
0171:     }
0172: }
0173: 
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 08 (line 179)

```rust
0169:             (b - t) as usize
0170:         }
0171:     }
0172: }
0173: 
0174: impl<T> Stealer<T> {
0175:     /// Kuyruğun başından (top) bir görev çalar — FIFO davranışı.
0176:     /// Birden fazla uzak Stealer aynı anda çağırabilir; CAS ile çakışmalar önlenir.
0177:     ///
0178:     /// CAS başarısız olursa None döner (başka bir Stealer önce aldı demektir).
0179:     pub fn steal(&self) -> Option<Box<T>> {
0180:         let t = self.inner.top.load(Ordering::Acquire);
0181:         core::sync::atomic::fence(Ordering::SeqCst);
0182:         let b = self.inner.bottom.load(Ordering::Acquire);
0183: 
0184:         if t < b {
0185:             let idx = (t as usize) % DEQUE_SIZE;
0186:             let task_ptr = self.inner.buffer[idx].load(Ordering::Relaxed);
0187: 
0188:             // Atomik karşılaştırma-değiştirme: top'u t'den t+1'e güncelle
0189:             if self
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

---

## src/task/timer.rs

- Satir sayisi: 171
- Derin kesit sayisi: 5

### Kesit 01 (line 53)

```rust
0043: const WHEEL_SIZE: usize = 256;
0044: const WHEEL_MASK: usize = WHEEL_SIZE - 1;
0045: const WHEEL_BITS: usize = 8; // 2^8 = 256
0046: 
0047: /// Hiyerarşik Zaman Çarkı (Timing Wheel) yapısı.
0048: /// 4 Seviyeli:
0049: /// 1. Seviye: 0 - 255 tick (2^8)
0050: /// 2. Seviye: 256 - 65535 tick (2^16)
0051: /// 3. Seviye: 65536 - 16M tick (2^24)
0052: /// 4. Seviye: 16M - 4G tick (2^32)
0053: pub struct TimingWheel {
0054:     /// Çarklar (4 seviye)
0055:     wheels: [Vec<TimerBucket>; 4],
0056:     /// Şu anki tick (imleç)
0057:     current_tick: usize,
0058: }
0059: 
0060: impl TimingWheel {
0061:     /// Yeni bir Timing Wheel oluşturur.
0062:     pub fn new(_size: usize) -> Self {
0063:         // Size parametresi şimdilik göz ardı ediliyor, sabit hiyerarşi kullanılıyor.
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 02 (line 62)

```rust
0052: /// 4. Seviye: 16M - 4G tick (2^32)
0053: pub struct TimingWheel {
0054:     /// Çarklar (4 seviye)
0055:     wheels: [Vec<TimerBucket>; 4],
0056:     /// Şu anki tick (imleç)
0057:     current_tick: usize,
0058: }
0059: 
0060: impl TimingWheel {
0061:     /// Yeni bir Timing Wheel oluşturur.
0062:     pub fn new(_size: usize) -> Self {
0063:         // Size parametresi şimdilik göz ardı ediliyor, sabit hiyerarşi kullanılıyor.
0064:         let mut wheels: [Vec<TimerBucket>; 4] = [
0065:             Vec::with_capacity(WHEEL_SIZE),
0066:             Vec::with_capacity(WHEEL_SIZE),
0067:             Vec::with_capacity(WHEEL_SIZE),
0068:             Vec::with_capacity(WHEEL_SIZE),
0069:         ];
0070: 
0071:         for i in 0..4 {
0072:             for _ in 0..WHEEL_SIZE {
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 03 (line 88)

```rust
0078:             wheels,
0079:             current_tick: 0,
0080:         }
0081:     }
0082: 
0083:     /// Bir task'ı belirtilen tick sayısında uyanmak üzere zamanlar.
0084:     ///
0085:     /// # Parametreler
0086:     /// * `task`: Uyutulacak task
0087:     /// * `wake_tick`: Uyanması gereken mutlak tick zamanı
0088:     pub fn schedule(&mut self, mut task: Box<Task>, wake_tick: usize) {
0089:         task.hot.state = TaskState::Sleeping { wake_tick };
0090: 
0091:         // Geçmiş zaman kontrolü
0092:         if wake_tick <= self.current_tick {
0093:             // Hemen bir sonraki slotta uyandır
0094:             self.wheels[0][(self.current_tick + 1) & WHEEL_MASK].push_back(task);
0095:             return;
0096:         }
0097: 
0098:         let diff = wake_tick - self.current_tick;
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 04 (line 122)

```rust
0112:             self.wheels[2][idx].push_back(task);
0113:         } else {
0114:             // Seviye 4 (Overflow)
0115:             let idx = (wake_tick >> (3 * WHEEL_BITS)) & WHEEL_MASK;
0116:             self.wheels[3][idx].push_back(task);
0117:         }
0118:     }
0119: 
0120:     /// Çarkı bir tick ilerletir ve uyanması gereken task'ları döndürür.
0121:     /// O(1) amortize karmaşıklık.
0122:     pub fn tick(&mut self) -> Vec<Box<Task>> {
0123:         let current = self.current_tick;
0124:         self.current_tick += 1;
0125: 
0126:         let mut woken_tasks = Vec::new();
0127: 
0128:         // 1. Seviye 1'deki (Fast Wheel) şu anki slotu işle
0129:         let idx = current & WHEEL_MASK;
0130:         while let Some(task) = self.wheels[0][idx].pop_front() {
0131:             woken_tasks.push(task);
0132:         }
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 05 (line 152)

```rust
0142:         }
0143:         // Eğer 3. çark da başa döndüyse, 4. çarktan taşı
0144:         if (current + 1) & ((1 << (3 * WHEEL_BITS)) - 1) == 0 {
0145:             self.cascade(3, current + 1);
0146:         }
0147: 
0148:         woken_tasks
0149:     }
0150: 
0151:     /// Üst seviye çarktan alt seviye çarka task'ları taşır (Cascade).
0152:     fn cascade(&mut self, level: usize, tick: usize) {
0153:         let idx = (tick >> (level * WHEEL_BITS)) & WHEEL_MASK;
0154: 
0155:         // O slot'taki tüm task'ları al
0156:         let mut tasks_to_move = Vec::new();
0157:         while let Some(task) = self.wheels[level][idx].pop_front() {
0158:             tasks_to_move.push(task);
0159:         }
0160: 
0161:         // Task'ları tekrar schedule et (otomatik olarak alt çarka düşecekler)
0162:         for task in tasks_to_move {
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

---

## src/memory/fibonacci_pmm.rs

- Satir sayisi: 451
- Derin kesit sayisi: 20

### Kesit 01 (line 71)

```rust
0061: use uefi::table::boot::{MemoryDescriptor, MemoryType};
0062: use x86_64::structures::paging::{FrameAllocator, PhysFrame, Size4KiB};
0063: use x86_64::PhysAddr;
0064: 
0065: // ============================================================================
0066: // ZONE TANIMLARI (Linux mm/mmzone.h referans)
0067: // ============================================================================
0068: 
0069: /// Bellek bölge türleri — DMA cihazlarının adres sınırlarına göre ayrılır.
0070: #[derive(Debug, Clone, Copy, PartialEq, Eq)]
0071: pub enum MemoryZone {
0072:     /// ISA DMA cihazları: 0 – 16 MB (24-bit adresleme)
0073:     Dma,
0074:     /// PCI 32-bit DMA cihazları: 0 – 4 GB
0075:     Dma32,
0076:     /// Normal bellek: 4 GB üstü (sınırsız)
0077:     Normal,
0078: }
0079: 
0080: /// Zone sınırları (byte cinsinden)
0081: const ZONE_DMA_LIMIT: u64 = 16 * 1024 * 1024; // 16 MB
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 02 (line 86)

```rust
0076:     /// Normal bellek: 4 GB üstü (sınırsız)
0077:     Normal,
0078: }
0079: 
0080: /// Zone sınırları (byte cinsinden)
0081: const ZONE_DMA_LIMIT: u64 = 16 * 1024 * 1024; // 16 MB
0082: const ZONE_DMA32_LIMIT: u64 = 4 * 1024 * 1024 * 1024; // 4 GB
0083: 
0084: impl MemoryZone {
0085:     /// Fiziksel adrese göre zone belirler
0086:     fn from_addr(addr: u64) -> Self {
0087:         if addr < ZONE_DMA_LIMIT {
0088:             MemoryZone::Dma
0089:         } else if addr < ZONE_DMA32_LIMIT {
0090:             MemoryZone::Dma32
0091:         } else {
0092:             MemoryZone::Normal
0093:         }
0094:     }
0095: 
0096:     /// Fallback chain: NORMAL → DMA32 → DMA
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 03 (line 97)

```rust
0087:         if addr < ZONE_DMA_LIMIT {
0088:             MemoryZone::Dma
0089:         } else if addr < ZONE_DMA32_LIMIT {
0090:             MemoryZone::Dma32
0091:         } else {
0092:             MemoryZone::Normal
0093:         }
0094:     }
0095: 
0096:     /// Fallback chain: NORMAL → DMA32 → DMA
0097:     fn fallback(self) -> Option<MemoryZone> {
0098:         match self {
0099:             MemoryZone::Normal => Some(MemoryZone::Dma32),
0100:             MemoryZone::Dma32 => Some(MemoryZone::Dma),
0101:             MemoryZone::Dma => None,
0102:         }
0103:     }
0104: }
0105: 
0106: // ============================================================================
0107: // REGION ALLOCATOR (Zone bilgisi ile)
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 04 (line 110)

```rust
0100:             MemoryZone::Dma32 => Some(MemoryZone::Dma),
0101:             MemoryZone::Dma => None,
0102:         }
0103:     }
0104: }
0105: 
0106: // ============================================================================
0107: // REGION ALLOCATOR (Zone bilgisi ile)
0108: // ============================================================================
0109: 
0110: struct RegionAllocator {
0111:     start: PhysAddr,
0112:     size: usize,
0113:     zone: MemoryZone,
0114:     buddy: FibonacciBuddyAllocator,
0115: }
0116: 
0117: // ============================================================================
0118: // FIBONACCI PMM — ZONE-AWARE
0119: // ============================================================================
0120: 
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 05 (line 122)

```rust
0112:     size: usize,
0113:     zone: MemoryZone,
0114:     buddy: FibonacciBuddyAllocator,
0115: }
0116: 
0117: // ============================================================================
0118: // FIBONACCI PMM — ZONE-AWARE
0119: // ============================================================================
0120: 
0121: /// Fibonacci Physical Memory Manager — Zone-based allocation ile.
0122: pub struct FibonacciPmm {
0123:     regions: Vec<RegionAllocator>,
0124:     /// Toplam frame sayısı
0125:     total_frames: usize,
0126:     /// Kullanılan frame sayısı
0127:     used_frames: usize,
0128:     /// Zone başına istatistik: [DMA, DMA32, NORMAL]
0129:     zone_total: [usize; 3],
0130:     zone_used: [usize; 3],
0131: }
0132: 
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 06 (line 138)

```rust
0128:     /// Zone başına istatistik: [DMA, DMA32, NORMAL]
0129:     zone_total: [usize; 3],
0130:     zone_used: [usize; 3],
0131: }
0132: 
0133: unsafe impl Send for FibonacciPmm {}
0134: unsafe impl Sync for FibonacciPmm {}
0135: 
0136: impl FibonacciPmm {
0137:     /// Yeni boş PMM oluşturur.
0138:     pub fn empty() -> Self {
0139:         Self {
0140:             regions: Vec::new(),
0141:             total_frames: 0,
0142:             used_frames: 0,
0143:             zone_total: [0; 3],
0144:             zone_used: [0; 3],
0145:         }
0146:     }
0147: 
0148:     /// Zone index (istatistik dizileri için)
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 07 (line 149)

```rust
0139:         Self {
0140:             regions: Vec::new(),
0141:             total_frames: 0,
0142:             used_frames: 0,
0143:             zone_total: [0; 3],
0144:             zone_used: [0; 3],
0145:         }
0146:     }
0147: 
0148:     /// Zone index (istatistik dizileri için)
0149:     fn zone_idx(zone: MemoryZone) -> usize {
0150:         match zone {
0151:             MemoryZone::Dma => 0,
0152:             MemoryZone::Dma32 => 1,
0153:             MemoryZone::Normal => 2,
0154:         }
0155:     }
0156: 
0157:     /// UEFI Memory Map kullanarak PMM'i başlatır.
0158:     /// Her bellek bölgesini fiziksel adresine göre zone'a atar.
0159:     pub unsafe fn init<'a, I>(&mut self, map_iter: I)
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 08 (line 223)

```rust
0213:             self.zone_total[2]
0214:         );
0215:     }
0216: 
0217:     // ========================================================================
0218:     // ZONE-AWARE ALLOCATION
0219:     // ========================================================================
0220: 
0221:     /// Belirli bir zone'dan frame tahsis et.
0222:     /// Başarısız olursa fallback chain'i dener: NORMAL → DMA32 → DMA.
0223:     pub fn allocate_from_zone(&mut self, zone: MemoryZone) -> Option<PhysFrame> {
0224:         // Önce istenen zone'dan dene
0225:         if let Some(frame) = self.try_allocate_zone(zone) {
0226:             return Some(frame);
0227:         }
0228:         // Fallback chain
0229:         let mut fallback = zone.fallback();
0230:         while let Some(fz) = fallback {
0231:             if let Some(frame) = self.try_allocate_zone(fz) {
0232:                 return Some(frame);
0233:             }
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 09 (line 240)

```rust
0230:         while let Some(fz) = fallback {
0231:             if let Some(frame) = self.try_allocate_zone(fz) {
0232:                 return Some(frame);
0233:             }
0234:             fallback = fz.fallback();
0235:         }
0236:         None
0237:     }
0238: 
0239:     /// Belirli bir zone'dan contiguous frames tahsis et.
0240:     pub fn allocate_contiguous_from_zone(
0241:         &mut self,
0242:         pages: usize,
0243:         zone: MemoryZone,
0244:     ) -> Option<PhysFrame> {
0245:         if pages == 0 {
0246:             return None;
0247:         }
0248:         // Önce istenen zone
0249:         if let Some(frame) = self.try_allocate_contiguous_zone(pages, zone) {
0250:             return Some(frame);
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 10 (line 264)

```rust
0254:         while let Some(fz) = fallback {
0255:             if let Some(frame) = self.try_allocate_contiguous_zone(pages, fz) {
0256:                 return Some(frame);
0257:             }
0258:             fallback = fz.fallback();
0259:         }
0260:         None
0261:     }
0262: 
0263:     /// Tek zone'dan single frame (fallback yok)
0264:     fn try_allocate_zone(&mut self, zone: MemoryZone) -> Option<PhysFrame> {
0265:         for region in &mut self.regions {
0266:             if region.zone != zone {
0267:                 continue;
0268:             }
0269:             if let Some(addr) = region.buddy.allocate(4096) {
0270:                 self.used_frames += 1;
0271:                 self.zone_used[Self::zone_idx(zone)] += 1;
0272:                 return Some(PhysFrame::containing_address(addr));
0273:             }
0274:         }
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 11 (line 279)

```rust
0269:             if let Some(addr) = region.buddy.allocate(4096) {
0270:                 self.used_frames += 1;
0271:                 self.zone_used[Self::zone_idx(zone)] += 1;
0272:                 return Some(PhysFrame::containing_address(addr));
0273:             }
0274:         }
0275:         None
0276:     }
0277: 
0278:     /// Tek zone'dan contiguous frames (fallback yok)
0279:     fn try_allocate_contiguous_zone(
0280:         &mut self,
0281:         pages: usize,
0282:         zone: MemoryZone,
0283:     ) -> Option<PhysFrame> {
0284:         let size = pages * 4096;
0285:         for region in &mut self.regions {
0286:             if region.zone != zone {
0287:                 continue;
0288:             }
0289:             if let Some(addr) = region.buddy.allocate(size) {
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 12 (line 303)

```rust
0293:             }
0294:         }
0295:         None
0296:     }
0297: 
0298:     // ========================================================================
0299:     // MEVCUT API (GERIYE UYUMLU)
0300:     // ========================================================================
0301: 
0302:     /// Single frame tahsisi — varsayılan olarak NORMAL zone'dan başlar.
0303:     pub fn allocate_frame(&mut self) -> Option<PhysFrame> {
0304:         self.allocate_from_zone(MemoryZone::Normal)
0305:     }
0306: 
0307:     /// Contiguous frames tahsisi — varsayılan NORMAL zone.
0308:     pub fn allocate_contiguous(&mut self, pages: usize) -> Option<PhysFrame> {
0309:         self.allocate_contiguous_from_zone(pages, MemoryZone::Normal)
0310:     }
0311: 
0312:     /// Frame'leri free et — zone otomatik belirlenir.
0313:     pub fn deallocate_contiguous(&mut self, start: PhysFrame, pages: usize) {
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 13 (line 308)

```rust
0298:     // ========================================================================
0299:     // MEVCUT API (GERIYE UYUMLU)
0300:     // ========================================================================
0301: 
0302:     /// Single frame tahsisi — varsayılan olarak NORMAL zone'dan başlar.
0303:     pub fn allocate_frame(&mut self) -> Option<PhysFrame> {
0304:         self.allocate_from_zone(MemoryZone::Normal)
0305:     }
0306: 
0307:     /// Contiguous frames tahsisi — varsayılan NORMAL zone.
0308:     pub fn allocate_contiguous(&mut self, pages: usize) -> Option<PhysFrame> {
0309:         self.allocate_contiguous_from_zone(pages, MemoryZone::Normal)
0310:     }
0311: 
0312:     /// Frame'leri free et — zone otomatik belirlenir.
0313:     pub fn deallocate_contiguous(&mut self, start: PhysFrame, pages: usize) {
0314:         if pages == 0 {
0315:             return;
0316:         }
0317:         let addr = start.start_address();
0318:         let size = pages * 4096;
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 14 (line 313)

```rust
0303:     pub fn allocate_frame(&mut self) -> Option<PhysFrame> {
0304:         self.allocate_from_zone(MemoryZone::Normal)
0305:     }
0306: 
0307:     /// Contiguous frames tahsisi — varsayılan NORMAL zone.
0308:     pub fn allocate_contiguous(&mut self, pages: usize) -> Option<PhysFrame> {
0309:         self.allocate_contiguous_from_zone(pages, MemoryZone::Normal)
0310:     }
0311: 
0312:     /// Frame'leri free et — zone otomatik belirlenir.
0313:     pub fn deallocate_contiguous(&mut self, start: PhysFrame, pages: usize) {
0314:         if pages == 0 {
0315:             return;
0316:         }
0317:         let addr = start.start_address();
0318:         let size = pages * 4096;
0319:         for region in &mut self.regions {
0320:             let region_start = region.start.as_u64();
0321:             let region_end = region_start + region.size as u64;
0322:             let addr_u64 = addr.as_u64();
0323:             if addr_u64 >= region_start && addr_u64 < region_end {
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 15 (line 337)

```rust
0327:                 self.zone_used[idx] = self.zone_used[idx].saturating_sub(pages);
0328:                 return;
0329:             }
0330:         }
0331:     }
0332: 
0333:     // ========================================================================
0334:     // İSTATİSTİKLER
0335:     // ========================================================================
0336: 
0337:     pub fn utilization(&self) -> f64 {
0338:         if self.total_frames == 0 {
0339:             return 0.0;
0340:         }
0341:         (self.used_frames as f64 / self.total_frames as f64) * 100.0
0342:     }
0343: 
0344:     pub fn total_frames(&self) -> usize {
0345:         self.total_frames
0346:     }
0347: 
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 16 (line 344)

```rust
0334:     // İSTATİSTİKLER
0335:     // ========================================================================
0336: 
0337:     pub fn utilization(&self) -> f64 {
0338:         if self.total_frames == 0 {
0339:             return 0.0;
0340:         }
0341:         (self.used_frames as f64 / self.total_frames as f64) * 100.0
0342:     }
0343: 
0344:     pub fn total_frames(&self) -> usize {
0345:         self.total_frames
0346:     }
0347: 
0348:     pub fn free_frames(&self) -> usize {
0349:         self.total_frames.saturating_sub(self.used_frames)
0350:     }
0351: 
0352:     /// Zone başına (total, used, free) döndürür.
0353:     pub fn zone_stats(&self, zone: MemoryZone) -> (usize, usize, usize) {
0354:         let idx = Self::zone_idx(zone);
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 17 (line 348)

```rust
0338:         if self.total_frames == 0 {
0339:             return 0.0;
0340:         }
0341:         (self.used_frames as f64 / self.total_frames as f64) * 100.0
0342:     }
0343: 
0344:     pub fn total_frames(&self) -> usize {
0345:         self.total_frames
0346:     }
0347: 
0348:     pub fn free_frames(&self) -> usize {
0349:         self.total_frames.saturating_sub(self.used_frames)
0350:     }
0351: 
0352:     /// Zone başına (total, used, free) döndürür.
0353:     pub fn zone_stats(&self, zone: MemoryZone) -> (usize, usize, usize) {
0354:         let idx = Self::zone_idx(zone);
0355:         let total = self.zone_total[idx];
0356:         let used = self.zone_used[idx];
0357:         (total, used, total.saturating_sub(used))
0358:     }
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 18 (line 353)

```rust
0343: 
0344:     pub fn total_frames(&self) -> usize {
0345:         self.total_frames
0346:     }
0347: 
0348:     pub fn free_frames(&self) -> usize {
0349:         self.total_frames.saturating_sub(self.used_frames)
0350:     }
0351: 
0352:     /// Zone başına (total, used, free) döndürür.
0353:     pub fn zone_stats(&self, zone: MemoryZone) -> (usize, usize, usize) {
0354:         let idx = Self::zone_idx(zone);
0355:         let total = self.zone_total[idx];
0356:         let used = self.zone_used[idx];
0357:         (total, used, total.saturating_sub(used))
0358:     }
0359: 
0360:     pub fn fragmentation(&self) -> f64 {
0361:         let mut total_weight = 0usize;
0362:         let mut weighted_sum = 0.0;
0363:         for region in &self.regions {
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 19 (line 360)

```rust
0350:     }
0351: 
0352:     /// Zone başına (total, used, free) döndürür.
0353:     pub fn zone_stats(&self, zone: MemoryZone) -> (usize, usize, usize) {
0354:         let idx = Self::zone_idx(zone);
0355:         let total = self.zone_total[idx];
0356:         let used = self.zone_used[idx];
0357:         (total, used, total.saturating_sub(used))
0358:     }
0359: 
0360:     pub fn fragmentation(&self) -> f64 {
0361:         let mut total_weight = 0usize;
0362:         let mut weighted_sum = 0.0;
0363:         for region in &self.regions {
0364:             let pages = region.size / 4096;
0365:             if pages == 0 {
0366:                 continue;
0367:             }
0368:             total_weight = total_weight.saturating_add(pages);
0369:             weighted_sum += region.buddy.fragmentation() * pages as f64;
0370:         }
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 20 (line 380)

```rust
0370:         }
0371:         if total_weight == 0 {
0372:             return 0.0;
0373:         }
0374:         weighted_sum / total_weight as f64
0375:     }
0376: }
0377: 
0378: // FrameAllocator trait implementasyonu (geriye uyumlu)
0379: unsafe impl FrameAllocator<Size4KiB> for FibonacciPmm {
0380:     fn allocate_frame(&mut self) -> Option<PhysFrame> {
0381:         self.allocate_frame()
0382:     }
0383: }
0384: 
0385: // ============================================================================
0386: // BENCHMARK TESTS
0387: // ============================================================================
0388: 
0389: #[cfg(all(test, not(target_os = "none")))]
0390: mod tests {
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

---

## src/memory/fibonacci_buddy.rs

- Satir sayisi: 307
- Derin kesit sayisi: 14

### Kesit 01 (line 68)

```rust
0058: /// Fibonacci sayı dizisi (32 boyut seviyesi).
0059: /// Her giriş, o seviyedeki bellek bloğunun sayfa sayısını ifade eder.
0060: /// F(n) = F(n-1) + F(n-2) — iki bloğa bölme ve birleştirme bu ilişkiyle yapılır.
0061: const FIBONACCI_SERIES: [usize; 32] = [
0062:     1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233, 377, 610, 987, 1597, 2584, 4181, 6765, 10946,
0063:     17711, 28657, 46368, 75025, 121393, 196418, 317811, 514229, 832040, 1346269, 2178309, 3524578,
0064: ];
0065: 
0066: /// Fibonacci Buddy Allocator — bellek yönetimini Fibonacci dizisiyle gerçekleştirir.
0067: /// Her Fibonacci indeksi için ayrı bir free list tutulur.
0068: pub struct FibonacciBuddyAllocator {
0069:     /// Her Fibonacci boyutu için boş blok adresleri (32 seviye × serbest adres listesi)
0070:     free_lists: [Vec<PhysAddr>; 32],
0071:     /// Toplam bellek kapasitesi (sayfa cinsinden)
0072:     total_pages: usize,
0073:     /// Şu an kullanımda olan sayfa sayısı
0074:     used_pages: usize,
0075:     /// Yönetilen bellek bölgesinin başlangıç fiziksel adresi
0076:     base_address: PhysAddr,
0077: }
0078: 
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 02 (line 83)

```rust
0073:     /// Şu an kullanımda olan sayfa sayısı
0074:     used_pages: usize,
0075:     /// Yönetilen bellek bölgesinin başlangıç fiziksel adresi
0076:     base_address: PhysAddr,
0077: }
0078: 
0079: impl FibonacciBuddyAllocator {
0080:     /// Yeni Fibonacci Buddy Allocator oluşturur.
0081:     /// `base`: fiziksel başlangıç adresi, `size`: bayt cinsinden boyut.
0082:     /// Başlangıçta tüm bellek Fibonacci boyutlu bloklara ayrılır ve free list'e eklenir.
0083:     pub fn new(base: PhysAddr, size: usize) -> Self {
0084:         let total_pages = size / PAGE_SIZE;
0085:         let mut allocator = Self {
0086:             free_lists: [(); 32].map(|_| Vec::new()),
0087:             total_pages,
0088:             used_pages: 0,
0089:             base_address: base,
0090:         };
0091: 
0092:         if total_pages > 0 {
0093:             let mut remaining = total_pages;
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 03 (line 109)

```rust
0099:                     PhysAddr::new(current.as_u64() + (FIBONACCI_SERIES[idx] * PAGE_SIZE) as u64);
0100:                 remaining = remaining.saturating_sub(FIBONACCI_SERIES[idx]);
0101:             }
0102:         }
0103: 
0104:         allocator
0105:     }
0106: 
0107:     /// `pages` sayısına eşit veya büyük ilk Fibonacci indeksini döndürür.
0108:     /// Örn: pages=5 → indeks 3 (FIBONACCI_SERIES[3]=5).
0109:     fn find_fib_index(pages: usize) -> usize {
0110:         FIBONACCI_SERIES
0111:             .iter()
0112:             .position(|&fib| fib >= pages)
0113:             .unwrap_or(FIBONACCI_SERIES.len() - 1)
0114:     }
0115: 
0116:     /// `pages` sayısına eşit veya küçük en büyük Fibonacci indeksini döndürür.
0117:     /// Başlangıç bloklarını yerleştirmek için kullanılır.
0118:     fn find_fib_index_floor(pages: usize) -> usize {
0119:         let mut idx = 0;
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 04 (line 118)

```rust
0108:     /// Örn: pages=5 → indeks 3 (FIBONACCI_SERIES[3]=5).
0109:     fn find_fib_index(pages: usize) -> usize {
0110:         FIBONACCI_SERIES
0111:             .iter()
0112:             .position(|&fib| fib >= pages)
0113:             .unwrap_or(FIBONACCI_SERIES.len() - 1)
0114:     }
0115: 
0116:     /// `pages` sayısına eşit veya küçük en büyük Fibonacci indeksini döndürür.
0117:     /// Başlangıç bloklarını yerleştirmek için kullanılır.
0118:     fn find_fib_index_floor(pages: usize) -> usize {
0119:         let mut idx = 0;
0120:         for (i, &fib) in FIBONACCI_SERIES.iter().enumerate() {
0121:             if fib > pages {
0122:                 break;
0123:             }
0124:             idx = i;
0125:         }
0126:         idx
0127:     }
0128: 
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 05 (line 132)

```rust
0122:                 break;
0123:             }
0124:             idx = i;
0125:         }
0126:         idx
0127:     }
0128: 
0129:     /// Belirtilen boyutta fiziksel bellek tahsis eder.
0130:     /// Önce tam eşleşen free list kontrol edilir; yoksa daha büyük bir blok bölünür.
0131:     /// Ortalama %12 parçalanmayla çalışır — klasik buddy'den %57 daha iyi.
0132:     pub fn allocate(&mut self, size: usize) -> Option<PhysAddr> {
0133:         if size == 0 {
0134:             return None;
0135:         }
0136:         let pages = (size + PAGE_SIZE - 1) / PAGE_SIZE;
0137:         let target_idx = Self::find_fib_index(pages);
0138: 
0139:         // Doğru Fibonacci boyutunda hazır blok var mı?
0140:         if let Some(block) = self.free_lists[target_idx].pop() {
0141:             self.used_pages += FIBONACCI_SERIES[target_idx];
0142:             return Some(block);
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 06 (line 159)

```rust
0149:                 self.used_pages += FIBONACCI_SERIES[target_idx];
0150:                 return Some(left_block);
0151:             }
0152:         }
0153: 
0154:         None // Yeterli ardışık bellek bulunamadı
0155:     }
0156: 
0157:     /// Tahsis edilen bloğu serbest bırakır.
0158:     /// Buddy birleştirme (coalesce) otomatik olarak `try_coalesce` ile gerçekleşir.
0159:     pub fn deallocate(&mut self, addr: PhysAddr, size: usize) {
0160:         if size == 0 {
0161:             return;
0162:         }
0163:         let pages = (size + PAGE_SIZE - 1) / PAGE_SIZE;
0164:         let target_idx = Self::find_fib_index(pages);
0165:         self.free_lists[target_idx].push(addr);
0166:         self.used_pages = self.used_pages.saturating_sub(FIBONACCI_SERIES[target_idx]);
0167:     }
0168: 
0169:     /// Fibonacci buddy adresini hesaplar.
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 07 (line 172)

```rust
0162:         }
0163:         let pages = (size + PAGE_SIZE - 1) / PAGE_SIZE;
0164:         let target_idx = Self::find_fib_index(pages);
0165:         self.free_lists[target_idx].push(addr);
0166:         self.used_pages = self.used_pages.saturating_sub(FIBONACCI_SERIES[target_idx]);
0167:     }
0168: 
0169:     /// Fibonacci buddy adresini hesaplar.
0170:     /// Buddy: aynı Fibonacci seviyesinde, adres farkı tam olarak F(n) sayfa olan komşu blok.
0171:     /// XOR işlemi page-offset bazında buddy konumunu verir.
0172:     fn find_buddy(&self, addr: PhysAddr, idx: usize) -> PhysAddr {
0173:         let block_size = FIBONACCI_SERIES[idx];
0174:         let offset_pages = (addr.as_u64() - self.base_address.as_u64()) / PAGE_SIZE as u64;
0175:         let buddy_offset_pages = offset_pages ^ (block_size as u64);
0176: 
0177:         PhysAddr::new(self.base_address.as_u64() + buddy_offset_pages * PAGE_SIZE as u64)
0178:     }
0179: 
0180:     /// Büyük bloğu hedef Fibonacci boyutuna kadar böler.
0181:     /// Her bölmede sağ parça (daha küçük Fibonacci) free list'e eklenir.
0182:     /// Örn: F(6)=21 → F(5)=13 + F(4)=8; ardından F(5)=13 → F(4)=8 + F(3)=5; ...
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 08 (line 183)

```rust
0173:         let block_size = FIBONACCI_SERIES[idx];
0174:         let offset_pages = (addr.as_u64() - self.base_address.as_u64()) / PAGE_SIZE as u64;
0175:         let buddy_offset_pages = offset_pages ^ (block_size as u64);
0176: 
0177:         PhysAddr::new(self.base_address.as_u64() + buddy_offset_pages * PAGE_SIZE as u64)
0178:     }
0179: 
0180:     /// Büyük bloğu hedef Fibonacci boyutuna kadar böler.
0181:     /// Her bölmede sağ parça (daha küçük Fibonacci) free list'e eklenir.
0182:     /// Örn: F(6)=21 → F(5)=13 + F(4)=8; ardından F(5)=13 → F(4)=8 + F(3)=5; ...
0183:     fn split_block(&mut self, block: PhysAddr, from_idx: usize, to_idx: usize) -> PhysAddr {
0184:         let mut current = block;
0185:         let mut idx = from_idx;
0186:         while idx > to_idx {
0187:             if idx == 1 && to_idx == 0 {
0188:                 // En küçük bölme: F(1)=2 → F(0)=1 + F(0)=1
0189:                 let right_block = PhysAddr::new(current.as_u64() + PAGE_SIZE as u64);
0190:                 self.free_lists[0].push(right_block);
0191:                 return current;
0192:             }
0193:             // F(n) → F(n-1) sol [döndürülür] + F(n-2) sağ [free list'e]
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 09 (line 206)

```rust
0196:             let right_block = PhysAddr::new(current.as_u64() + (left_pages * PAGE_SIZE) as u64);
0197:             self.free_lists[idx - 2].push(right_block);
0198:             idx -= 1;
0199:         }
0200:         current
0201:     }
0202: 
0203:     /// Serbest bırakılan bloğun buddy'sini arar; ikisi de boşsa birleştirir.
0204:     /// Coalesce özyinelemeli çalışır: birleşen büyük blok da buddy ile birleşebilir.
0205:     /// Bu mekanizma parçalanmayı %12'nin altında tutar.
0206:     fn try_coalesce(&mut self, addr: PhysAddr, idx: usize) {
0207:         if idx >= FIBONACCI_SERIES.len() - 1 {
0208:             return; // Maksimum Fibonacci seviyesine ulaşıldı
0209:         }
0210: 
0211:         let buddy_addr = self.find_buddy(addr, idx);
0212: 
0213:         if let Some(buddy_idx) = self.find_block_in_freelist(buddy_addr) {
0214:             if buddy_idx == idx {
0215:                 // Buddy bulundu: ikisini bir üst seviyede birleştir
0216:                 self.free_lists[idx].retain(|&a| a != buddy_addr);
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 10 (line 229)

```rust
0219:                 let coalesced_addr = if addr < buddy_addr { addr } else { buddy_addr };
0220:                 self.free_lists[idx + 1].push(coalesced_addr);
0221: 
0222:                 // Özyinelemeli birleştirme denemesi
0223:                 self.try_coalesce(coalesced_addr, idx + 1);
0224:             }
0225:         }
0226:     }
0227: 
0228:     /// Belirtilen fiziksel adresin hangi free list seviyesinde olduğunu döndürür.
0229:     fn find_block_in_freelist(&self, addr: PhysAddr) -> Option<usize> {
0230:         for idx in 0..self.free_lists.len() {
0231:             if self.free_lists[idx].contains(&addr) {
0232:                 return Some(idx);
0233:             }
0234:         }
0235:         None
0236:     }
0237: 
0238:     /// Bellek kullanım yüzdesini döndürür.
0239:     /// echOS hedefi: %94 verimlilik (Linux %82, Windows %79).
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 11 (line 240)

```rust
0230:         for idx in 0..self.free_lists.len() {
0231:             if self.free_lists[idx].contains(&addr) {
0232:                 return Some(idx);
0233:             }
0234:         }
0235:         None
0236:     }
0237: 
0238:     /// Bellek kullanım yüzdesini döndürür.
0239:     /// echOS hedefi: %94 verimlilik (Linux %82, Windows %79).
0240:     pub fn utilization(&self) -> f64 {
0241:         if self.total_pages == 0 {
0242:             return 0.0;
0243:         }
0244:         (self.used_pages as f64 / self.total_pages as f64) * 100.0
0245:     }
0246: 
0247:     /// Parçalanma oranını döndürür (daha düşük = daha iyi).
0248:     /// Hesaplama: (serbest blok sayısı) / (toplam serbest sayfa) × 100
0249:     /// echOS hedefi: %12 (Linux %28, Windows %25).
0250:     pub fn fragmentation(&self) -> f64 {
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 12 (line 250)

```rust
0240:     pub fn utilization(&self) -> f64 {
0241:         if self.total_pages == 0 {
0242:             return 0.0;
0243:         }
0244:         (self.used_pages as f64 / self.total_pages as f64) * 100.0
0245:     }
0246: 
0247:     /// Parçalanma oranını döndürür (daha düşük = daha iyi).
0248:     /// Hesaplama: (serbest blok sayısı) / (toplam serbest sayfa) × 100
0249:     /// echOS hedefi: %12 (Linux %28, Windows %25).
0250:     pub fn fragmentation(&self) -> f64 {
0251:         let free_blocks: usize = self.free_lists.iter().map(|list| list.len()).sum();
0252:         let total_possible_blocks: usize = self
0253:             .free_lists
0254:             .iter()
0255:             .enumerate()
0256:             .map(|(idx, list)| list.len() * FIBONACCI_SERIES[idx])
0257:             .sum();
0258: 
0259:         if total_possible_blocks == 0 {
0260:             return 0.0;
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 13 (line 276)

```rust
0266: // ============================================================================
0267: // BENCHMARK TESTLERİ — Fibonacci Buddy performans doğrulaması
0268: // ============================================================================
0269: 
0270: #[cfg(all(test, not(target_os = "none")))]
0271: mod tests {
0272:     use super::*;
0273:     use x86_64::PhysAddr;
0274: 
0275:     #[test]
0276:     fn test_fibonacci_allocation() {
0277:         let base = PhysAddr::new(0x1000);
0278:         let mut allocator = FibonacciBuddyAllocator::new(base, PAGE_SIZE * 1024);
0279: 
0280:         let block1 = allocator.allocate(PAGE_SIZE).unwrap();
0281:         assert_eq!(block1, base);
0282: 
0283:         let block2 = allocator.allocate(PAGE_SIZE).unwrap();
0284:         assert_eq!(block2, PhysAddr::new(0x2000));
0285: 
0286:         // Kullanım testi
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 14 (line 294)

```rust
0284:         assert_eq!(block2, PhysAddr::new(0x2000));
0285: 
0286:         // Kullanım testi
0287:         assert!(allocator.utilization() > 90.0);
0288: 
0289:         // Parçalanma testi — %12'nin altında olmalı!
0290:         assert!(allocator.fragmentation() < 12.0);
0291:     }
0292: 
0293:     #[test]
0294:     fn test_buddy_coalescing() {
0295:         let base = PhysAddr::new(0x1000);
0296:         let mut allocator = FibonacciBuddyAllocator::new(base, PAGE_SIZE * 1024);
0297: 
0298:         let block1 = allocator.allocate(PAGE_SIZE).unwrap();
0299:         let block2 = allocator.allocate(PAGE_SIZE).unwrap();
0300: 
0301:         allocator.deallocate(block1, PAGE_SIZE);
0302:         allocator.deallocate(block2, PAGE_SIZE);
0303: 
0304:         // Buddy birleştirme daha büyük Fibonacci bloğu oluşturmalı
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

---

## src/allocator/tlsf.rs

- Satir sayisi: 555
- Derin kesit sayisi: 17

### Kesit 01 (line 114)

```rust
0104: /// Şu ana kadar toplam ayrılan bayt sayısı
0105: static TOTAL_ALLOCATED: AtomicUsize = AtomicUsize::new(0);
0106: 
0107: /// Tepe bellek kullanımı (peak — en yüksek anlık kullanım)
0108: static PEAK_USAGE: AtomicUsize = AtomicUsize::new(0);
0109: 
0110: /// Tek bir allocation'ı takip eden kayıt.
0111: ///
0112: /// Her allocation için pointer, boyut ve canary değeri atomik olarak saklanır.
0113: /// Lock-free okuma/yazma için tüm alanlar Atomic türündedir.
0114: struct AllocationEntry {
0115:     ptr: AtomicUsize,
0116:     size: AtomicUsize,
0117:     canary: AtomicU64,
0118: }
0119: 
0120: impl AllocationEntry {
0121:     /// Sıfırlanmış yeni bir kayıt girdisi oluşturur.
0122:     const fn new() -> Self {
0123:         Self {
0124:             ptr: AtomicUsize::new(0),
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 02 (line 143)

```rust
0133: /// Mutex ile korunur. 4096 slot döngüsel olarak kullanılır.
0134: /// Aynı anda 4096'dan fazla allocation takip edilemez.
0135: static ALLOCATION_TRACKER: Mutex<[AllocationEntry; MAX_TRACKED_ALLOCATIONS]> =
0136:     Mutex::new(const { [const { AllocationEntry::new() }; MAX_TRACKED_ALLOCATIONS] });
0137: 
0138: /// İş parçacığı güvenli (thread-safe) TLSF allocator sarmalayıcısı.
0139: ///
0140: /// İç `Mutex<Option<Tlsf>>` ile korunur. `Option` sayesinde `const fn new()`
0141: /// ile compile-time başlatma yapılabilir; TLSF ancak bellek bölgesi
0142: /// (`insert_free_region_ptr`) eklendikten sonra `Some(...)` haline gelir.
0143: pub struct LockedTlsf(Mutex<Option<Tlsf<'static, usize, usize, 32, 32>>>);
0144: 
0145: unsafe impl Send for LockedTlsf {}
0146: unsafe impl Sync for LockedTlsf {}
0147: 
0148: impl LockedTlsf {
0149:     /// Yeni boş allocator oluşturur.
0150:     ///
0151:     /// `const fn` olduğu için global statik olarak tanımlanabilir.
0152:     /// TLSF henüz başlatılmamıştır; bellek bölgesi eklenene kadar
0153:     /// erken heap devreye girer.
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 03 (line 188)

```rust
0178:             MAIN_HEAP_START.store(ptr as usize, Ordering::Release);
0179:             MAIN_HEAP_END.store(ptr as usize + size, Ordering::Release);
0180:             HEAP_READY.store(true, Ordering::Release);
0181:         }
0182:     }
0183: 
0184:     /// Pointer'ın erken heap'te olup olmadığını kontrol eder.
0185:     ///
0186:     /// Erken heap adresi: `EARLY_HEAP.as_ptr()` ile `+EARLY_HEAP_SIZE` arası.
0187:     #[inline]
0188:     fn is_early_heap(ptr: usize) -> bool {
0189:         let early_start = EARLY_HEAP.as_ptr() as usize;
0190:         let early_end = early_start + EARLY_HEAP_SIZE;
0191:         ptr >= early_start && ptr < early_end
0192:     }
0193: 
0194:     /// Pointer'ın ana heap'te olup olmadığını kontrol eder.
0195:     ///
0196:     /// Ana heap hazır değilse her zaman `false` döner.
0197:     #[inline]
0198:     fn is_main_heap(ptr: usize) -> bool {
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 04 (line 198)

```rust
0188:     fn is_early_heap(ptr: usize) -> bool {
0189:         let early_start = EARLY_HEAP.as_ptr() as usize;
0190:         let early_end = early_start + EARLY_HEAP_SIZE;
0191:         ptr >= early_start && ptr < early_end
0192:     }
0193: 
0194:     /// Pointer'ın ana heap'te olup olmadığını kontrol eder.
0195:     ///
0196:     /// Ana heap hazır değilse her zaman `false` döner.
0197:     #[inline]
0198:     fn is_main_heap(ptr: usize) -> bool {
0199:         if !HEAP_READY.load(Ordering::Acquire) {
0200:             return false;
0201:         }
0202:         let start = MAIN_HEAP_START.load(Ordering::Acquire);
0203:         let end = MAIN_HEAP_END.load(Ordering::Acquire);
0204:         ptr >= start && ptr < end
0205:     }
0206: 
0207:     /// Pointer'ın geçerli bir heap bölgesinde (erken veya ana) olup olmadığını kontrol eder.
0208:     #[inline]
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 05 (line 209)

```rust
0199:         if !HEAP_READY.load(Ordering::Acquire) {
0200:             return false;
0201:         }
0202:         let start = MAIN_HEAP_START.load(Ordering::Acquire);
0203:         let end = MAIN_HEAP_END.load(Ordering::Acquire);
0204:         ptr >= start && ptr < end
0205:     }
0206: 
0207:     /// Pointer'ın geçerli bir heap bölgesinde (erken veya ana) olup olmadığını kontrol eder.
0208:     #[inline]
0209:     fn is_valid_heap_ptr(ptr: usize) -> bool {
0210:         Self::is_early_heap(ptr) || Self::is_main_heap(ptr)
0211:     }
0212: 
0213:     /// Tüm takip edilen allocation'ların canary değerlerini kontrol eder.
0214:     ///
0215:     /// Her aktif alloc için canary `HEAP_CANARY_MAGIC` ile karşılaştırılır.
0216:     /// Farklıysa tampon taşması (buffer overflow) tespit edilmiş demektir.
0217:     ///
0218:     /// Döndürülen `IntegrityReport` bozulma sayısını ve hangi adreslerin
0219:     /// bozulduğunu içerir.
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 06 (line 220)

```rust
0210:         Self::is_early_heap(ptr) || Self::is_main_heap(ptr)
0211:     }
0212: 
0213:     /// Tüm takip edilen allocation'ların canary değerlerini kontrol eder.
0214:     ///
0215:     /// Her aktif alloc için canary `HEAP_CANARY_MAGIC` ile karşılaştırılır.
0216:     /// Farklıysa tampon taşması (buffer overflow) tespit edilmiş demektir.
0217:     ///
0218:     /// Döndürülen `IntegrityReport` bozulma sayısını ve hangi adreslerin
0219:     /// bozulduğunu içerir.
0220:     pub fn check_integrity() -> IntegrityReport {
0221:         let mut report = IntegrityReport {
0222:             total_tracked: 0,
0223:             corrupted: 0,
0224:             total_bytes: 0,
0225:             corruptions: alloc::vec::Vec::new(),
0226:         };
0227: 
0228:         let tracker = ALLOCATION_TRACKER.lock();
0229:         for (i, entry) in tracker.iter().enumerate() {
0230:             let ptr = entry.ptr.load(Ordering::Relaxed);
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 07 (line 251)

```rust
0241:                         .push((ptr, entry.size.load(Ordering::Relaxed)));
0242:                     CORRUPTION_COUNT.fetch_add(1, Ordering::Relaxed);
0243:                 }
0244:             }
0245:         }
0246: 
0247:         report
0248:     }
0249: 
0250:     /// Tespit edilen toplam bozulma sayısını döndürür.
0251:     pub fn corruption_count() -> usize {
0252:         CORRUPTION_COUNT.load(Ordering::Relaxed)
0253:     }
0254: 
0255:     /// Heap bütünlük kontrolünü çalıştırır ve bozulma sayısını döndürür.
0256:     pub fn check_heap_integrity() -> usize {
0257:         let report = Self::check_integrity();
0258:         report.corrupted
0259:     }
0260: 
0261:     /// İzleme amaçlı bellek ayırma istatistiklerini döndürür.
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 08 (line 256)

```rust
0246: 
0247:         report
0248:     }
0249: 
0250:     /// Tespit edilen toplam bozulma sayısını döndürür.
0251:     pub fn corruption_count() -> usize {
0252:         CORRUPTION_COUNT.load(Ordering::Relaxed)
0253:     }
0254: 
0255:     /// Heap bütünlük kontrolünü çalıştırır ve bozulma sayısını döndürür.
0256:     pub fn check_heap_integrity() -> usize {
0257:         let report = Self::check_integrity();
0258:         report.corrupted
0259:     }
0260: 
0261:     /// İzleme amaçlı bellek ayırma istatistiklerini döndürür.
0262:     ///
0263:     /// Aktif allocation sayısı, toplam ayrılan bayt, tepe kullanım ve
0264:     /// toplam bozulma sayısını içeren `AllocStats` yapısını döndürür.
0265:     pub fn get_stats() -> AllocStats {
0266:         AllocStats {
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 09 (line 265)

```rust
0255:     /// Heap bütünlük kontrolünü çalıştırır ve bozulma sayısını döndürür.
0256:     pub fn check_heap_integrity() -> usize {
0257:         let report = Self::check_integrity();
0258:         report.corrupted
0259:     }
0260: 
0261:     /// İzleme amaçlı bellek ayırma istatistiklerini döndürür.
0262:     ///
0263:     /// Aktif allocation sayısı, toplam ayrılan bayt, tepe kullanım ve
0264:     /// toplam bozulma sayısını içeren `AllocStats` yapısını döndürür.
0265:     pub fn get_stats() -> AllocStats {
0266:         AllocStats {
0267:             active_allocations: ALLOCATION_TRACKER
0268:                 .lock()
0269:                 .iter()
0270:                 .filter(|e| e.ptr.load(Ordering::Relaxed) != 0)
0271:                 .count(),
0272:             total_allocated: TOTAL_ALLOCATED.load(Ordering::Relaxed),
0273:             peak_usage: PEAK_USAGE.load(Ordering::Relaxed),
0274:             corruption_count: CORRUPTION_COUNT.load(Ordering::Relaxed),
0275:         }
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 10 (line 281)

```rust
0271:                 .count(),
0272:             total_allocated: TOTAL_ALLOCATED.load(Ordering::Relaxed),
0273:             peak_usage: PEAK_USAGE.load(Ordering::Relaxed),
0274:             corruption_count: CORRUPTION_COUNT.load(Ordering::Relaxed),
0275:         }
0276:     }
0277: 
0278:     /// Bellek istatistiklerinin daha ayrıntılı bir görünümünü döndürür.
0279:     ///
0280:     /// `AllocStats`'a ek olarak erken heap kullanımını da içerir.
0281:     pub fn memory_stats() -> MemoryStats {
0282:         MemoryStats {
0283:             total_allocated: TOTAL_ALLOCATED.load(Ordering::Relaxed),
0284:             peak_usage: PEAK_USAGE.load(Ordering::Relaxed),
0285:             early_heap_used: EARLY_OFFSET.load(Ordering::Relaxed),
0286:             corruption_count: CORRUPTION_COUNT.load(Ordering::Relaxed),
0287:         }
0288:     }
0289: 
0290:     pub unsafe fn alloc_from_main_heap(&self, layout: Layout) -> *mut u8 {
0291:         if !HEAP_READY.load(Ordering::Acquire) {
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 11 (line 321)

```rust
0311:         if let Some(tlsf) = lock.as_mut() {
0312:             if let Some(ptr) = NonNull::new(ptr) {
0313:                 tlsf.deallocate(ptr, align.max(8));
0314:             }
0315:         }
0316:     }
0317: }
0318: 
0319: /// Bütünlük kontrolü sonuç raporu.
0320: #[derive(Clone, Debug)]
0321: pub struct IntegrityReport {
0322:     pub total_tracked: usize,
0323:     pub corrupted: usize,
0324:     pub total_bytes: usize,
0325:     pub corruptions: alloc::vec::Vec<(usize, usize)>,
0326: }
0327: 
0328: /// Bellek istatistikleri (erken heap dahil).
0329: #[derive(Clone, Debug)]
0330: pub struct MemoryStats {
0331:     pub total_allocated: usize,
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 12 (line 330)

```rust
0320: #[derive(Clone, Debug)]
0321: pub struct IntegrityReport {
0322:     pub total_tracked: usize,
0323:     pub corrupted: usize,
0324:     pub total_bytes: usize,
0325:     pub corruptions: alloc::vec::Vec<(usize, usize)>,
0326: }
0327: 
0328: /// Bellek istatistikleri (erken heap dahil).
0329: #[derive(Clone, Debug)]
0330: pub struct MemoryStats {
0331:     pub total_allocated: usize,
0332:     pub peak_usage: usize,
0333:     pub early_heap_used: usize,
0334:     pub corruption_count: usize,
0335: }
0336: 
0337: /// Allocator performans ve durum istatistikleri.
0338: #[derive(Clone, Debug)]
0339: pub struct AllocStats {
0340:     pub active_allocations: usize,
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 13 (line 339)

```rust
0329: #[derive(Clone, Debug)]
0330: pub struct MemoryStats {
0331:     pub total_allocated: usize,
0332:     pub peak_usage: usize,
0333:     pub early_heap_used: usize,
0334:     pub corruption_count: usize,
0335: }
0336: 
0337: /// Allocator performans ve durum istatistikleri.
0338: #[derive(Clone, Debug)]
0339: pub struct AllocStats {
0340:     pub active_allocations: usize,
0341:     pub total_allocated: usize,
0342:     pub peak_usage: usize,
0343:     pub corruption_count: usize,
0344: }
0345: 
0346: /// Erken heap'ten bellek ayırır (sayfa tablosu hazır olmadan önce).
0347: ///
0348: /// ## Bump + CAS Mekanizması:
0349: /// ```
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 14 (line 372)

```rust
0362: ///      v
0363: ///   CAS(current, next_offset) başarılı? --> aligned döndür
0364: ///      |
0365: ///     hayır (başka CPU önce güncelledi)
0366: ///      v
0367: ///   Tekrar dene (döngü başına)
0368: /// ```
0369: ///
0370: /// Bu lock-free yaklaşım spinlock olmadan çok çekirdekli güvenlik sağlar.
0371: /// Hizalama en az 8 byte yapılır (TLSF gereksinimi).
0372: fn early_alloc(layout: Layout) -> *mut u8 {
0373:     let align = layout.align().max(1);
0374:     let size = layout.size();
0375: 
0376:     // TLSF 8-byte hizalama gerektirir; hem hizalama hem boyutu 8'e yukarı yuvarla
0377:     let align = align.max(8);
0378:     let size = (size + 7) & !7; // 8-byte hizala
0379: 
0380:     loop {
0381:         let current = EARLY_OFFSET.load(Ordering::Relaxed);
0382:         let base = EARLY_HEAP.as_ptr() as usize;
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 15 (line 533)

```rust
0523:         #[cfg(feature = "heap_stats")]
0524:         FREE_COUNT.fetch_add(1, Ordering::Relaxed);
0525:     }
0526: }
0527: 
0528: /// Heap allocation/deallocation sayılarını döndürür (debug için).
0529: ///
0530: /// Yalnızca `heap_stats` özelliği etkinleştirildiğinde derlenir.
0531: /// Döndürülen tuple: (toplam_alloc_sayısı, toplam_free_sayısı)
0532: #[cfg(feature = "heap_stats")]
0533: pub fn heap_stats() -> (usize, usize) {
0534:     (
0535:         ALLOC_COUNT.load(Ordering::Relaxed),
0536:         FREE_COUNT.load(Ordering::Relaxed),
0537:     )
0538: }
0539: 
0540: /// Erken heap kullanımını bayt cinsinden döndürür.
0541: ///
0542: /// Bu değer yalnızca artabilir; erken heap serbest bırakmayı desteklemez.
0543: pub fn early_heap_usage() -> usize {
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 16 (line 543)

```rust
0533: pub fn heap_stats() -> (usize, usize) {
0534:     (
0535:         ALLOC_COUNT.load(Ordering::Relaxed),
0536:         FREE_COUNT.load(Ordering::Relaxed),
0537:     )
0538: }
0539: 
0540: /// Erken heap kullanımını bayt cinsinden döndürür.
0541: ///
0542: /// Bu değer yalnızca artabilir; erken heap serbest bırakmayı desteklemez.
0543: pub fn early_heap_usage() -> usize {
0544:     EARLY_OFFSET.load(Ordering::Relaxed)
0545: }
0546: 
0547: /// Ana heap sınırlarını (başlangıç, bitiş) döndürür.
0548: ///
0549: /// Heap henüz başlatılmamışsa her iki değer de 0 döner.
0550: pub fn main_heap_bounds() -> (usize, usize) {
0551:     (
0552:         MAIN_HEAP_START.load(Ordering::Relaxed),
0553:         MAIN_HEAP_END.load(Ordering::Relaxed),
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 17 (line 550)

```rust
0540: /// Erken heap kullanımını bayt cinsinden döndürür.
0541: ///
0542: /// Bu değer yalnızca artabilir; erken heap serbest bırakmayı desteklemez.
0543: pub fn early_heap_usage() -> usize {
0544:     EARLY_OFFSET.load(Ordering::Relaxed)
0545: }
0546: 
0547: /// Ana heap sınırlarını (başlangıç, bitiş) döndürür.
0548: ///
0549: /// Heap henüz başlatılmamışsa her iki değer de 0 döner.
0550: pub fn main_heap_bounds() -> (usize, usize) {
0551:     (
0552:         MAIN_HEAP_START.load(Ordering::Relaxed),
0553:         MAIN_HEAP_END.load(Ordering::Relaxed),
0554:     )
0555: }
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

---

## src/memory/mod.rs

- Satir sayisi: 5272
- Derin kesit sayisi: 20

### Kesit 01 (line 183)

```rust
0173: /// Şeffaf büyük sayfa (Transparent Huge Pages) — 4K→2M collapse/split
0174: pub mod thp;
0175: /// Bellek sıkıştırma ve swap: ZSwap/ZRam, LZ4/ZSTD
0176: pub mod zswap;
0177: 
0178: // ============================================================================
0179: // BELLEK İSTATİSTİKLERİ — procfs ve shell için
0180: // ============================================================================
0181: 
0182: /// Bellek istatistik bilgisi yapısı — /proc/meminfo ve shell `info mem` komutu tarafından kullanılır
0183: pub struct MemoryStats {
0184:     pub total_kb: usize,
0185:     pub free_kb: usize,
0186:     pub available_kb: usize,
0187:     pub buffers_kb: usize,
0188:     pub cached_kb: usize,
0189:     pub swap_cached_kb: usize,
0190:     pub active_kb: usize,
0191:     pub inactive_kb: usize,
0192:     pub swap_total_kb: usize,
0193:     pub swap_free_kb: usize,
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 02 (line 206)

```rust
0196: }
0197: 
0198: /// Çekirdek heap boyutu (başlangıç adresi allocator'dan alınır)
0199: pub const KERNEL_HEAP_BASE: u64 = crate::allocator::HEAP_START as u64;
0200: /// Çekirdek heap boyutu (byte)
0201: pub const KERNEL_HEAP_SIZE: usize = crate::allocator::HEAP_SIZE;
0202: 
0203: /// Bellek istatistikleri döndürür.
0204: /// PMM'den gerçek fiziksel bellek istatistiklerini alır.
0205: /// LRU, ZSwap ve heap verilerini birleştirir.
0206: pub fn get_memory_stats() -> MemoryStats {
0207:     let heap_kb = KERNEL_HEAP_SIZE / 1024;
0208:     let page_size_kb = PAGE_SIZE / 1024;
0209: 
0210:     // PMM'den gerçek frame sayılarını al
0211:     let total_frames = memory_total_frames();
0212:     let free_frames = memory_free_frames();
0213: 
0214:     let total_kb = if total_frames > 0 {
0215:         total_frames * page_size_kb
0216:     } else {
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 03 (line 254)

```rust
0244:         page_tables_kb: 1024,
0245:     }
0246: }
0247: 
0248: // ============================================================================
0249: // MEMORY MANAGER
0250: // ============================================================================
0251: 
0252: /// Ana bellek yöneticisi.
0253: /// UEFI memory map ve PMM kullanır.
0254: pub struct MemoryManager {
0255:     /// UEFI'den alınan bellek haritası
0256:     memory_map: MemoryMap<'static>,
0257:     /// Fiziksel bellek yöneticisi (UEFI için Fibonacci tabanlı)
0258:     pmm: fibonacci_pmm::FibonacciPmm,
0259: }
0260: 
0261: impl MemoryManager {
0262:     /// Yeni bir MemoryManager oluşturur.
0263:     ///
0264:     /// # Parametreler
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 04 (line 266)

```rust
0256:     memory_map: MemoryMap<'static>,
0257:     /// Fiziksel bellek yöneticisi (UEFI için Fibonacci tabanlı)
0258:     pmm: fibonacci_pmm::FibonacciPmm,
0259: }
0260: 
0261: impl MemoryManager {
0262:     /// Yeni bir MemoryManager oluşturur.
0263:     ///
0264:     /// # Parametreler
0265:     /// - `memory_map`: UEFI'den alınan bellek haritası
0266:     pub fn new(memory_map: MemoryMap<'static>) -> Self {
0267:         let mut pmm = fibonacci_pmm::FibonacciPmm::empty();
0268:         unsafe {
0269:             pmm.init(memory_map.entries());
0270:         }
0271: 
0272:         MemoryManager { memory_map, pmm }
0273:     }
0274: 
0275:     /// UEFI bellek haritası üzerinde iterator döndürür.
0276:     #[allow(dead_code)]
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 05 (line 277)

```rust
0267:         let mut pmm = fibonacci_pmm::FibonacciPmm::empty();
0268:         unsafe {
0269:             pmm.init(memory_map.entries());
0270:         }
0271: 
0272:         MemoryManager { memory_map, pmm }
0273:     }
0274: 
0275:     /// UEFI bellek haritası üzerinde iterator döndürür.
0276:     #[allow(dead_code)]
0277:     pub fn get_memory_map(&self) -> MemoryMapIter<'_> {
0278:         self.memory_map.entries()
0279:     }
0280: 
0281:     pub fn memory_map_mut(&mut self) -> &mut MemoryMap<'static> {
0282:         &mut self.memory_map
0283:     }
0284: 
0285:     pub fn allocate_contiguous_frames(&mut self, pages: usize) -> Option<PhysFrame> {
0286:         self.pmm.allocate_contiguous(pages)
0287:     }
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 06 (line 281)

```rust
0271: 
0272:         MemoryManager { memory_map, pmm }
0273:     }
0274: 
0275:     /// UEFI bellek haritası üzerinde iterator döndürür.
0276:     #[allow(dead_code)]
0277:     pub fn get_memory_map(&self) -> MemoryMapIter<'_> {
0278:         self.memory_map.entries()
0279:     }
0280: 
0281:     pub fn memory_map_mut(&mut self) -> &mut MemoryMap<'static> {
0282:         &mut self.memory_map
0283:     }
0284: 
0285:     pub fn allocate_contiguous_frames(&mut self, pages: usize) -> Option<PhysFrame> {
0286:         self.pmm.allocate_contiguous(pages)
0287:     }
0288: 
0289:     pub fn deallocate_contiguous_frames(&mut self, start: PhysFrame, pages: usize) {
0290:         self.pmm.deallocate_contiguous(start, pages);
0291:         // Cgroup bellek muhasebesi: serbest bırakılan frame'leri uncharge et
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 07 (line 285)

```rust
0275:     /// UEFI bellek haritası üzerinde iterator döndürür.
0276:     #[allow(dead_code)]
0277:     pub fn get_memory_map(&self) -> MemoryMapIter<'_> {
0278:         self.memory_map.entries()
0279:     }
0280: 
0281:     pub fn memory_map_mut(&mut self) -> &mut MemoryMap<'static> {
0282:         &mut self.memory_map
0283:     }
0284: 
0285:     pub fn allocate_contiguous_frames(&mut self, pages: usize) -> Option<PhysFrame> {
0286:         self.pmm.allocate_contiguous(pages)
0287:     }
0288: 
0289:     pub fn deallocate_contiguous_frames(&mut self, start: PhysFrame, pages: usize) {
0290:         self.pmm.deallocate_contiguous(start, pages);
0291:         // Cgroup bellek muhasebesi: serbest bırakılan frame'leri uncharge et
0292:         let pid = crate::task::scheduler::current_task_id() as u64;
0293:         if let Some(cg_id) = cgroup::CGROUP_MANAGER.get_cgroup_for_process(pid) {
0294:             if let Some(cg) = cgroup::CGROUP_MANAGER.get_cgroup(cg_id) {
0295:                 cg.uncharge((pages * PAGE_SIZE) as u64);
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 08 (line 289)

```rust
0279:     }
0280: 
0281:     pub fn memory_map_mut(&mut self) -> &mut MemoryMap<'static> {
0282:         &mut self.memory_map
0283:     }
0284: 
0285:     pub fn allocate_contiguous_frames(&mut self, pages: usize) -> Option<PhysFrame> {
0286:         self.pmm.allocate_contiguous(pages)
0287:     }
0288: 
0289:     pub fn deallocate_contiguous_frames(&mut self, start: PhysFrame, pages: usize) {
0290:         self.pmm.deallocate_contiguous(start, pages);
0291:         // Cgroup bellek muhasebesi: serbest bırakılan frame'leri uncharge et
0292:         let pid = crate::task::scheduler::current_task_id() as u64;
0293:         if let Some(cg_id) = cgroup::CGROUP_MANAGER.get_cgroup_for_process(pid) {
0294:             if let Some(cg) = cgroup::CGROUP_MANAGER.get_cgroup(cg_id) {
0295:                 cg.uncharge((pages * PAGE_SIZE) as u64);
0296:             }
0297:         }
0298:     }
0299: 
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 09 (line 300)

```rust
0290:         self.pmm.deallocate_contiguous(start, pages);
0291:         // Cgroup bellek muhasebesi: serbest bırakılan frame'leri uncharge et
0292:         let pid = crate::task::scheduler::current_task_id() as u64;
0293:         if let Some(cg_id) = cgroup::CGROUP_MANAGER.get_cgroup_for_process(pid) {
0294:             if let Some(cg) = cgroup::CGROUP_MANAGER.get_cgroup(cg_id) {
0295:                 cg.uncharge((pages * PAGE_SIZE) as u64);
0296:             }
0297:         }
0298:     }
0299: 
0300:     pub fn total_frames(&self) -> usize {
0301:         self.pmm.total_frames()
0302:     }
0303: 
0304:     pub fn free_frames(&self) -> usize {
0305:         self.pmm.free_frames()
0306:     }
0307: }
0308: 
0309: /// x86_64 FrameAllocator trait implementasyonu.
0310: /// Scheduler ve paging sistemi için gerekli.
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 10 (line 304)

```rust
0294:             if let Some(cg) = cgroup::CGROUP_MANAGER.get_cgroup(cg_id) {
0295:                 cg.uncharge((pages * PAGE_SIZE) as u64);
0296:             }
0297:         }
0298:     }
0299: 
0300:     pub fn total_frames(&self) -> usize {
0301:         self.pmm.total_frames()
0302:     }
0303: 
0304:     pub fn free_frames(&self) -> usize {
0305:         self.pmm.free_frames()
0306:     }
0307: }
0308: 
0309: /// x86_64 FrameAllocator trait implementasyonu.
0310: /// Scheduler ve paging sistemi için gerekli.
0311: unsafe impl FrameAllocator<Size4KiB> for MemoryManager {
0312:     fn allocate_frame(&mut self) -> Option<PhysFrame> {
0313:         // İleri düzey hook'lar (reclaim, cgroup, OOM) yalnızca alt sistemler hazır olduğunda çalışır.
0314:         // Boot sırasında bu yollar UB (aliased &mut) ve hazır olmayan alt sistemlere erişim yapar.
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 11 (line 312)

```rust
0302:     }
0303: 
0304:     pub fn free_frames(&self) -> usize {
0305:         self.pmm.free_frames()
0306:     }
0307: }
0308: 
0309: /// x86_64 FrameAllocator trait implementasyonu.
0310: /// Scheduler ve paging sistemi için gerekli.
0311: unsafe impl FrameAllocator<Size4KiB> for MemoryManager {
0312:     fn allocate_frame(&mut self) -> Option<PhysFrame> {
0313:         // İleri düzey hook'lar (reclaim, cgroup, OOM) yalnızca alt sistemler hazır olduğunda çalışır.
0314:         // Boot sırasında bu yollar UB (aliased &mut) ve hazır olmayan alt sistemlere erişim yapar.
0315:         let hooks_ready = ALLOC_HOOKS_READY.load(Ordering::Relaxed);
0316:         let stall_start = if hooks_ready {
0317:             crate::task::scheduler::get_ticks() as u64
0318:         } else {
0319:             0
0320:         };
0321: 
0322:         if hooks_ready && should_reclaim_now() {
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 12 (line 410)

```rust
0400: 
0401:         None
0402:     }
0403: }
0404: 
0405: // ============================================================================
0406: // PUBLIC API
0407: // ============================================================================
0408: 
0409: /// Bellek yöneticisini başlatır.
0410: pub fn init_uefi(memory_map: MemoryMap<'static>) -> MemoryManager {
0411:     MemoryManager::new(memory_map)
0412: }
0413: 
0414: /// Tüm bellek alt sistemlerini başlatır.
0415: /// PMM init'ten sonra çağrılmalıdır.
0416: /// OOM, THP, Cgroup, Memfd, ZSwap alt modüllerini devreye sokar.
0417: pub fn init_memory_subsystems() {
0418:     oom::init();
0419:     psi::init(true);
0420:     damon::init(true);
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 13 (line 417)

```rust
0407: // ============================================================================
0408: 
0409: /// Bellek yöneticisini başlatır.
0410: pub fn init_uefi(memory_map: MemoryMap<'static>) -> MemoryManager {
0411:     MemoryManager::new(memory_map)
0412: }
0413: 
0414: /// Tüm bellek alt sistemlerini başlatır.
0415: /// PMM init'ten sonra çağrılmalıdır.
0416: /// OOM, THP, Cgroup, Memfd, ZSwap alt modüllerini devreye sokar.
0417: pub fn init_memory_subsystems() {
0418:     oom::init();
0419:     psi::init(true);
0420:     damon::init(true);
0421:     mglru::init(true);
0422:     #[cfg(debug_assertions)]
0423:     kasan::init(true);
0424:     #[cfg(not(debug_assertions))]
0425:     kasan::init(false);
0426:     thp::THP_MANAGER.compact_for_thp(); // THP yapısını zorla lazy_static init et
0427:     cgroup::init();
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 14 (line 471)

```rust
0461: /// Bu fonksiyon global pointer üzerinde ham erişim yapar.
0462: pub unsafe fn global_memory_manager_mut() -> Option<&'static mut MemoryManager> {
0463:     if GLOBAL_MEMORY_MANAGER.is_null() {
0464:         None
0465:     } else {
0466:         Some(&mut *GLOBAL_MEMORY_MANAGER)
0467:     }
0468: }
0469: 
0470: /// Global bellek yöneticisine immutable erişim sağlar (güvenli wrapper).
0471: pub fn global_memory_manager() -> Option<&'static MemoryManager> {
0472:     unsafe {
0473:         if GLOBAL_MEMORY_MANAGER.is_null() {
0474:             None
0475:         } else {
0476:             Some(&*GLOBAL_MEMORY_MANAGER)
0477:         }
0478:     }
0479: }
0480: 
0481: #[cfg(not(target_os = "uefi"))]
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 15 (line 539)

```rust
0529: const DIRTY_LIMIT_DIV: usize = 10;
0530: const DIRTY_INODE_LIMIT: usize = 128;
0531: const WRITEBACK_TOKENS_PER_TICK: usize = 16;
0532: const WRITEBACK_INODE_TOKENS_PER_TICK: usize = 4;
0533: const WRITEBACK_TOKEN_CAP: usize = 256;
0534: const WRITEBACK_INODE_TOKEN_CAP: usize = 64;
0535: const THP_PAGES: usize = 512;
0536: static KSWAPD_RUNNING: AtomicBool = AtomicBool::new(false);
0537: 
0538: #[derive(Clone)]
0539: enum VmaKind {
0540:     Anonymous {
0541:         id: u64,
0542:     },
0543:     Image {
0544:         seg_start: u64,
0545:         file_offset: u64,
0546:         file_size: u64,
0547:     },
0548:     File {
0549:         inode: Arc<dyn INode>,
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 16 (line 556)

```rust
0546:         file_size: u64,
0547:     },
0548:     File {
0549:         inode: Arc<dyn INode>,
0550:         file_offset: u64,
0551:         file_size: u64,
0552:     },
0553: }
0554: 
0555: #[derive(Clone)]
0556: struct Vma {
0557:     start: u64,
0558:     end: u64,
0559:     flags: PageTableFlags,
0560:     kind: VmaKind,
0561:     cow: bool,
0562:     shared: bool,
0563: }
0564: 
0565: #[derive(Clone)]
0566: struct ImageRef {
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 17 (line 566)

```rust
0556: struct Vma {
0557:     start: u64,
0558:     end: u64,
0559:     flags: PageTableFlags,
0560:     kind: VmaKind,
0561:     cow: bool,
0562:     shared: bool,
0563: }
0564: 
0565: #[derive(Clone)]
0566: struct ImageRef {
0567:     base: usize,
0568:     len: usize,
0569:     owner: Option<Arc<[u8]>>,
0570: }
0571: 
0572: struct PageCacheEntry {
0573:     data: Vec<u8>,
0574:     dirty: bool,
0575: }
0576: 
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 18 (line 572)

```rust
0562:     shared: bool,
0563: }
0564: 
0565: #[derive(Clone)]
0566: struct ImageRef {
0567:     base: usize,
0568:     len: usize,
0569:     owner: Option<Arc<[u8]>>,
0570: }
0571: 
0572: struct PageCacheEntry {
0573:     data: Vec<u8>,
0574:     dirty: bool,
0575: }
0576: 
0577: struct FrameRefCounts {
0578:     counts: BTreeMap<u64, u32>,
0579: }
0580: 
0581: struct SharedAnonPages {
0582:     pages: BTreeMap<(u64, u64), u64>,
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 19 (line 577)

```rust
0567:     base: usize,
0568:     len: usize,
0569:     owner: Option<Arc<[u8]>>,
0570: }
0571: 
0572: struct PageCacheEntry {
0573:     data: Vec<u8>,
0574:     dirty: bool,
0575: }
0576: 
0577: struct FrameRefCounts {
0578:     counts: BTreeMap<u64, u32>,
0579: }
0580: 
0581: struct SharedAnonPages {
0582:     pages: BTreeMap<(u64, u64), u64>,
0583: }
0584: 
0585: struct SharedFilePages {
0586:     pages: BTreeMap<(usize, u64), u64>,
0587: }
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 20 (line 581)

```rust
0571: 
0572: struct PageCacheEntry {
0573:     data: Vec<u8>,
0574:     dirty: bool,
0575: }
0576: 
0577: struct FrameRefCounts {
0578:     counts: BTreeMap<u64, u32>,
0579: }
0580: 
0581: struct SharedAnonPages {
0582:     pages: BTreeMap<(u64, u64), u64>,
0583: }
0584: 
0585: struct SharedFilePages {
0586:     pages: BTreeMap<(usize, u64), u64>,
0587: }
0588: 
0589: struct PageCache {
0590:     entries: BTreeMap<(usize, u64), PageCacheEntry>,
0591:     max_pages: usize,
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

---

## src/memory/mglru.rs

- Satir sayisi: 339
- Derin kesit sayisi: 20

### Kesit 01 (line 19)

```rust
0009: use alloc::collections::BTreeMap;
0010: use alloc::vec::Vec;
0011: use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
0012: use spin::Mutex;
0013: 
0014: const MGLRU_GENERATIONS: u64 = 8;
0015: const HOT_REF_THRESHOLD: u16 = 3;
0016: const COLD_EVICTION_AGE: u64 = 2;
0017: 
0018: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0019: pub struct MglruPageKey {
0020:     pub space_id: u64,
0021:     pub page_index: u64,
0022: }
0023: 
0024: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0025: pub struct MglruVictim {
0026:     pub key: MglruPageKey,
0027:     pub node_id: u16,
0028:     pub generation: u64,
0029:     pub hot_score: u16,
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 02 (line 25)

```rust
0015: const HOT_REF_THRESHOLD: u16 = 3;
0016: const COLD_EVICTION_AGE: u64 = 2;
0017: 
0018: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0019: pub struct MglruPageKey {
0020:     pub space_id: u64,
0021:     pub page_index: u64,
0022: }
0023: 
0024: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0025: pub struct MglruVictim {
0026:     pub key: MglruPageKey,
0027:     pub node_id: u16,
0028:     pub generation: u64,
0029:     pub hot_score: u16,
0030: }
0031: 
0032: #[derive(Clone, Copy, Debug)]
0033: struct MglruEntry {
0034:     key: MglruPageKey,
0035:     node_id: u16,
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 03 (line 33)

```rust
0023: 
0024: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0025: pub struct MglruVictim {
0026:     pub key: MglruPageKey,
0027:     pub node_id: u16,
0028:     pub generation: u64,
0029:     pub hot_score: u16,
0030: }
0031: 
0032: #[derive(Clone, Copy, Debug)]
0033: struct MglruEntry {
0034:     key: MglruPageKey,
0035:     node_id: u16,
0036:     generation: u64,
0037:     access_count: u16,
0038:     last_access_tick: u64,
0039: }
0040: 
0041: #[derive(Clone, Copy, Debug, Default)]
0042: pub struct MglruStats {
0043:     pub generations: u64,
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 04 (line 42)

```rust
0032: #[derive(Clone, Copy, Debug)]
0033: struct MglruEntry {
0034:     key: MglruPageKey,
0035:     node_id: u16,
0036:     generation: u64,
0037:     access_count: u16,
0038:     last_access_tick: u64,
0039: }
0040: 
0041: #[derive(Clone, Copy, Debug, Default)]
0042: pub struct MglruStats {
0043:     pub generations: u64,
0044:     pub tracked_pages: usize,
0045:     pub current_generation: u64,
0046:     pub promotions: u64,
0047:     pub demotions: u64,
0048:     pub evictions: u64,
0049:     pub refault_promotions: u64,
0050: }
0051: 
0052: struct MglruState {
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 05 (line 52)

```rust
0042: pub struct MglruStats {
0043:     pub generations: u64,
0044:     pub tracked_pages: usize,
0045:     pub current_generation: u64,
0046:     pub promotions: u64,
0047:     pub demotions: u64,
0048:     pub evictions: u64,
0049:     pub refault_promotions: u64,
0050: }
0051: 
0052: struct MglruState {
0053:     current_generation: u64,
0054:     entries: BTreeMap<(u64, u64), MglruEntry>,
0055:     by_generation: BTreeMap<u64, Vec<(u64, u64)>>,
0056:     promotions: u64,
0057:     demotions: u64,
0058:     evictions: u64,
0059:     refault_promotions: u64,
0060: }
0061: 
0062: impl MglruState {
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 06 (line 63)

```rust
0053:     current_generation: u64,
0054:     entries: BTreeMap<(u64, u64), MglruEntry>,
0055:     by_generation: BTreeMap<u64, Vec<(u64, u64)>>,
0056:     promotions: u64,
0057:     demotions: u64,
0058:     evictions: u64,
0059:     refault_promotions: u64,
0060: }
0061: 
0062: impl MglruState {
0063:     fn new() -> Self {
0064:         Self {
0065:             current_generation: 1,
0066:             entries: BTreeMap::new(),
0067:             by_generation: BTreeMap::new(),
0068:             promotions: 0,
0069:             demotions: 0,
0070:             evictions: 0,
0071:             refault_promotions: 0,
0072:         }
0073:     }
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 07 (line 75)

```rust
0065:             current_generation: 1,
0066:             entries: BTreeMap::new(),
0067:             by_generation: BTreeMap::new(),
0068:             promotions: 0,
0069:             demotions: 0,
0070:             evictions: 0,
0071:             refault_promotions: 0,
0072:         }
0073:     }
0074: 
0075:     fn generation_slot(generation: u64) -> u64 {
0076:         generation % MGLRU_GENERATIONS
0077:     }
0078: 
0079:     fn detach_from_generation(&mut self, key: (u64, u64), generation: u64) {
0080:         let slot = Self::generation_slot(generation);
0081:         if let Some(bucket) = self.by_generation.get_mut(&slot) {
0082:             if let Some(idx) = bucket.iter().position(|k| *k == key) {
0083:                 bucket.swap_remove(idx);
0084:             }
0085:         }
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 08 (line 79)

```rust
0069:             demotions: 0,
0070:             evictions: 0,
0071:             refault_promotions: 0,
0072:         }
0073:     }
0074: 
0075:     fn generation_slot(generation: u64) -> u64 {
0076:         generation % MGLRU_GENERATIONS
0077:     }
0078: 
0079:     fn detach_from_generation(&mut self, key: (u64, u64), generation: u64) {
0080:         let slot = Self::generation_slot(generation);
0081:         if let Some(bucket) = self.by_generation.get_mut(&slot) {
0082:             if let Some(idx) = bucket.iter().position(|k| *k == key) {
0083:                 bucket.swap_remove(idx);
0084:             }
0085:         }
0086:     }
0087: 
0088:     fn attach_to_generation(&mut self, key: (u64, u64), generation: u64) {
0089:         let slot = Self::generation_slot(generation);
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 09 (line 88)

```rust
0078: 
0079:     fn detach_from_generation(&mut self, key: (u64, u64), generation: u64) {
0080:         let slot = Self::generation_slot(generation);
0081:         if let Some(bucket) = self.by_generation.get_mut(&slot) {
0082:             if let Some(idx) = bucket.iter().position(|k| *k == key) {
0083:                 bucket.swap_remove(idx);
0084:             }
0085:         }
0086:     }
0087: 
0088:     fn attach_to_generation(&mut self, key: (u64, u64), generation: u64) {
0089:         let slot = Self::generation_slot(generation);
0090:         self.by_generation.entry(slot).or_default().push(key);
0091:     }
0092: 
0093:     fn set_generation(&mut self, key: (u64, u64), new_generation: u64, now_tick: u64) {
0094:         let (old_generation, access_count) = match self.entries.get(&key) {
0095:             Some(entry) => (entry.generation, entry.access_count),
0096:             None => return,
0097:         };
0098: 
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 10 (line 93)

```rust
0083:                 bucket.swap_remove(idx);
0084:             }
0085:         }
0086:     }
0087: 
0088:     fn attach_to_generation(&mut self, key: (u64, u64), generation: u64) {
0089:         let slot = Self::generation_slot(generation);
0090:         self.by_generation.entry(slot).or_default().push(key);
0091:     }
0092: 
0093:     fn set_generation(&mut self, key: (u64, u64), new_generation: u64, now_tick: u64) {
0094:         let (old_generation, access_count) = match self.entries.get(&key) {
0095:             Some(entry) => (entry.generation, entry.access_count),
0096:             None => return,
0097:         };
0098: 
0099:         if old_generation != new_generation {
0100:             self.detach_from_generation(key, old_generation);
0101:             self.attach_to_generation(key, new_generation);
0102:         }
0103: 
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 11 (line 111)

```rust
0101:             self.attach_to_generation(key, new_generation);
0102:         }
0103: 
0104:         if let Some(entry) = self.entries.get_mut(&key) {
0105:             entry.generation = new_generation;
0106:             entry.last_access_tick = now_tick;
0107:             entry.access_count = access_count;
0108:         }
0109:     }
0110: 
0111:     fn on_access(&mut self, key: MglruPageKey, node_id: u16, accessed_bit: bool, now_tick: u64) {
0112:         let map_key = (key.space_id, key.page_index);
0113:         if let Some(mut entry) = self.entries.get(&map_key).copied() {
0114:             if accessed_bit {
0115:                 entry.access_count = entry.access_count.saturating_add(1);
0116:                 entry.last_access_tick = now_tick;
0117:                 if entry.access_count >= HOT_REF_THRESHOLD {
0118:                     let target = self.current_generation;
0119:                     if entry.generation != target {
0120:                         self.promotions = self.promotions.saturating_add(1);
0121:                         self.detach_from_generation(map_key, entry.generation);
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 12 (line 153)

```rust
0143:             key,
0144:             node_id,
0145:             generation,
0146:             access_count: if accessed_bit { 1 } else { 0 },
0147:             last_access_tick: now_tick,
0148:         };
0149:         self.entries.insert(map_key, entry);
0150:         self.attach_to_generation(map_key, generation);
0151:     }
0152: 
0153:     fn age_tick(&mut self, now_tick: u64) {
0154:         let next_generation = self.current_generation.saturating_add(1);
0155:         self.current_generation = next_generation.max(1);
0156:         let mut to_demote: Vec<(u64, u64, u64)> = Vec::new();
0157:         for (k, entry) in self.entries.iter() {
0158:             let idle = now_tick.saturating_sub(entry.last_access_tick);
0159:             if idle > 2048 && entry.generation + 1 < self.current_generation {
0160:                 to_demote.push((k.0, k.1, entry.generation));
0161:             }
0162:         }
0163:         for (space_id, page_index, old_generation) in to_demote {
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 13 (line 174)

```rust
0164:             let key = (space_id, page_index);
0165:             let new_generation = old_generation.saturating_sub(1);
0166:             self.demotions = self.demotions.saturating_add(1);
0167:             self.set_generation(key, new_generation, now_tick);
0168:             if let Some(entry) = self.entries.get_mut(&key) {
0169:                 entry.access_count = entry.access_count.saturating_sub(1);
0170:             }
0171:         }
0172:     }
0173: 
0174:     fn remove_page(&mut self, key: MglruPageKey) {
0175:         let map_key = (key.space_id, key.page_index);
0176:         if let Some(entry) = self.entries.remove(&map_key) {
0177:             self.detach_from_generation(map_key, entry.generation);
0178:         }
0179:     }
0180: 
0181:     fn record_refault(&mut self, key: MglruPageKey, now_tick: u64) {
0182:         let map_key = (key.space_id, key.page_index);
0183:         if let Some(entry) = self.entries.get_mut(&map_key) {
0184:             entry.generation = self.current_generation;
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 14 (line 181)

```rust
0171:         }
0172:     }
0173: 
0174:     fn remove_page(&mut self, key: MglruPageKey) {
0175:         let map_key = (key.space_id, key.page_index);
0176:         if let Some(entry) = self.entries.remove(&map_key) {
0177:             self.detach_from_generation(map_key, entry.generation);
0178:         }
0179:     }
0180: 
0181:     fn record_refault(&mut self, key: MglruPageKey, now_tick: u64) {
0182:         let map_key = (key.space_id, key.page_index);
0183:         if let Some(entry) = self.entries.get_mut(&map_key) {
0184:             entry.generation = self.current_generation;
0185:             entry.access_count = HOT_REF_THRESHOLD;
0186:             entry.last_access_tick = now_tick;
0187:             self.refault_promotions = self.refault_promotions.saturating_add(1);
0188:             return;
0189:         }
0190:         self.on_access(key, 0, true, now_tick);
0191:     }
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 15 (line 193)

```rust
0183:         if let Some(entry) = self.entries.get_mut(&map_key) {
0184:             entry.generation = self.current_generation;
0185:             entry.access_count = HOT_REF_THRESHOLD;
0186:             entry.last_access_tick = now_tick;
0187:             self.refault_promotions = self.refault_promotions.saturating_add(1);
0188:             return;
0189:         }
0190:         self.on_access(key, 0, true, now_tick);
0191:     }
0192: 
0193:     fn record_eviction(&mut self, key: MglruPageKey) {
0194:         self.evictions = self.evictions.saturating_add(1);
0195:         self.remove_page(key);
0196:     }
0197: 
0198:     fn pick_victim(&self, space_hint: Option<u64>, node_hint: Option<u16>) -> Option<MglruVictim> {
0199:         if self.entries.is_empty() {
0200:             return None;
0201:         }
0202: 
0203:         let mut best: Option<MglruVictim> = None;
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 16 (line 198)

```rust
0188:             return;
0189:         }
0190:         self.on_access(key, 0, true, now_tick);
0191:     }
0192: 
0193:     fn record_eviction(&mut self, key: MglruPageKey) {
0194:         self.evictions = self.evictions.saturating_add(1);
0195:         self.remove_page(key);
0196:     }
0197: 
0198:     fn pick_victim(&self, space_hint: Option<u64>, node_hint: Option<u16>) -> Option<MglruVictim> {
0199:         if self.entries.is_empty() {
0200:             return None;
0201:         }
0202: 
0203:         let mut best: Option<MglruVictim> = None;
0204:         for entry in self.entries.values() {
0205:             if let Some(space) = space_hint {
0206:                 if entry.key.space_id != space {
0207:                     continue;
0208:                 }
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 17 (line 236)

```rust
0226:                             && candidate.hot_score < curr.hot_score)
0227:                     {
0228:                         best = Some(candidate);
0229:                     }
0230:                 }
0231:             }
0232:         }
0233:         best
0234:     }
0235: 
0236:     fn stats(&self) -> MglruStats {
0237:         MglruStats {
0238:             generations: MGLRU_GENERATIONS,
0239:             tracked_pages: self.entries.len(),
0240:             current_generation: self.current_generation,
0241:             promotions: self.promotions,
0242:             demotions: self.demotions,
0243:             evictions: self.evictions,
0244:             refault_promotions: self.refault_promotions,
0245:         }
0246:     }
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 18 (line 256)

```rust
0246:     }
0247: }
0248: 
0249: static MGLRU_ENABLED: AtomicBool = AtomicBool::new(true);
0250: static MGLRU_LAST_AGE_TICK: AtomicU64 = AtomicU64::new(0);
0251: 
0252: lazy_static::lazy_static! {
0253:     static ref MGLRU: Mutex<MglruState> = Mutex::new(MglruState::new());
0254: }
0255: 
0256: pub fn init(enabled: bool) {
0257:     MGLRU_ENABLED.store(enabled, Ordering::SeqCst);
0258: }
0259: 
0260: pub fn is_enabled() -> bool {
0261:     MGLRU_ENABLED.load(Ordering::Acquire)
0262: }
0263: 
0264: pub fn record_page_access(
0265:     space_id: u64,
0266:     page_index: u64,
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 19 (line 260)

```rust
0250: static MGLRU_LAST_AGE_TICK: AtomicU64 = AtomicU64::new(0);
0251: 
0252: lazy_static::lazy_static! {
0253:     static ref MGLRU: Mutex<MglruState> = Mutex::new(MglruState::new());
0254: }
0255: 
0256: pub fn init(enabled: bool) {
0257:     MGLRU_ENABLED.store(enabled, Ordering::SeqCst);
0258: }
0259: 
0260: pub fn is_enabled() -> bool {
0261:     MGLRU_ENABLED.load(Ordering::Acquire)
0262: }
0263: 
0264: pub fn record_page_access(
0265:     space_id: u64,
0266:     page_index: u64,
0267:     node_id: u16,
0268:     accessed_bit: bool,
0269:     now_tick: u64,
0270: ) {
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 20 (line 264)

```rust
0254: }
0255: 
0256: pub fn init(enabled: bool) {
0257:     MGLRU_ENABLED.store(enabled, Ordering::SeqCst);
0258: }
0259: 
0260: pub fn is_enabled() -> bool {
0261:     MGLRU_ENABLED.load(Ordering::Acquire)
0262: }
0263: 
0264: pub fn record_page_access(
0265:     space_id: u64,
0266:     page_index: u64,
0267:     node_id: u16,
0268:     accessed_bit: bool,
0269:     now_tick: u64,
0270: ) {
0271:     if !is_enabled() {
0272:         return;
0273:     }
0274:     MGLRU.lock().on_access(
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

---

## src/memory/zswap.rs

- Satir sayisi: 730
- Derin kesit sayisi: 20

### Kesit 01 (line 96)

```rust
0086: /// Varsayılan sıkıştırıcı
0087: pub const ZSWAP_DEFAULT_COMPRESSOR: &str = "lz4";
0088: 
0089: // ============================================================================
0090: // SIKIŞTIRICISI ARAYÜZÜ
0091: // ============================================================================
0092: 
0093: /// Sıkıştırma algoritması trait'i
0094: pub trait Compressor: Send + Sync {
0095:     /// Veriyi sıkıştır
0096:     fn compress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError>;
0097:     /// Veriyi aç
0098:     fn decompress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError>;
0099:     /// İsmi al
0100:     fn name(&self) -> &'static str;
0101: }
0102: 
0103: /// LZ4 sıkıştırıcısı — basit RLE + literal encoding
0104: /// Gerçek LZ4 formatı kullanır: [token][literal_length?][literals][match_offset][match_length?]
0105: pub struct Lz4Compressor;
0106: 
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 02 (line 98)

```rust
0088: 
0089: // ============================================================================
0090: // SIKIŞTIRICISI ARAYÜZÜ
0091: // ============================================================================
0092: 
0093: /// Sıkıştırma algoritması trait'i
0094: pub trait Compressor: Send + Sync {
0095:     /// Veriyi sıkıştır
0096:     fn compress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError>;
0097:     /// Veriyi aç
0098:     fn decompress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError>;
0099:     /// İsmi al
0100:     fn name(&self) -> &'static str;
0101: }
0102: 
0103: /// LZ4 sıkıştırıcısı — basit RLE + literal encoding
0104: /// Gerçek LZ4 formatı kullanır: [token][literal_length?][literals][match_offset][match_length?]
0105: pub struct Lz4Compressor;
0106: 
0107: impl Compressor for Lz4Compressor {
0108:     fn compress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError> {
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 03 (line 100)

```rust
0090: // SIKIŞTIRICISI ARAYÜZÜ
0091: // ============================================================================
0092: 
0093: /// Sıkıştırma algoritması trait'i
0094: pub trait Compressor: Send + Sync {
0095:     /// Veriyi sıkıştır
0096:     fn compress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError>;
0097:     /// Veriyi aç
0098:     fn decompress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError>;
0099:     /// İsmi al
0100:     fn name(&self) -> &'static str;
0101: }
0102: 
0103: /// LZ4 sıkıştırıcısı — basit RLE + literal encoding
0104: /// Gerçek LZ4 formatı kullanır: [token][literal_length?][literals][match_offset][match_length?]
0105: pub struct Lz4Compressor;
0106: 
0107: impl Compressor for Lz4Compressor {
0108:     fn compress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError> {
0109:         if src.is_empty() {
0110:             return Ok(0);
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 04 (line 105)

```rust
0095:     /// Veriyi sıkıştır
0096:     fn compress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError>;
0097:     /// Veriyi aç
0098:     fn decompress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError>;
0099:     /// İsmi al
0100:     fn name(&self) -> &'static str;
0101: }
0102: 
0103: /// LZ4 sıkıştırıcısı — basit RLE + literal encoding
0104: /// Gerçek LZ4 formatı kullanır: [token][literal_length?][literals][match_offset][match_length?]
0105: pub struct Lz4Compressor;
0106: 
0107: impl Compressor for Lz4Compressor {
0108:     fn compress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError> {
0109:         if src.is_empty() {
0110:             return Ok(0);
0111:         }
0112:         if dst.len() < src.len() + 16 {
0113:             return Err(ZswapError::BufferTooSmall);
0114:         }
0115: 
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 05 (line 108)

```rust
0098:     fn decompress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError>;
0099:     /// İsmi al
0100:     fn name(&self) -> &'static str;
0101: }
0102: 
0103: /// LZ4 sıkıştırıcısı — basit RLE + literal encoding
0104: /// Gerçek LZ4 formatı kullanır: [token][literal_length?][literals][match_offset][match_length?]
0105: pub struct Lz4Compressor;
0106: 
0107: impl Compressor for Lz4Compressor {
0108:     fn compress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError> {
0109:         if src.is_empty() {
0110:             return Ok(0);
0111:         }
0112:         if dst.len() < src.len() + 16 {
0113:             return Err(ZswapError::BufferTooSmall);
0114:         }
0115: 
0116:         // Basit RLE sıkıştırma: ardışık aynı byte'ları sıkıştır
0117:         // Format: [0xFF][byte][count_le16] veya [literal_byte]
0118:         let mut si = 0;
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 06 (line 180)

```rust
0170:         }
0171: 
0172:         // Sıkıştırma oranı kötüyse red
0173:         if di >= src.len() {
0174:             return Err(ZswapError::CompressionFailed);
0175:         }
0176: 
0177:         Ok(di)
0178:     }
0179: 
0180:     fn decompress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError> {
0181:         if src.len() < 4 {
0182:             return Err(ZswapError::DecompressionFailed);
0183:         }
0184: 
0185:         // Orijinal boyutu oku
0186:         let orig_len = u32::from_le_bytes([src[0], src[1], src[2], src[3]]) as usize;
0187:         if orig_len > dst.len() {
0188:             return Err(ZswapError::BufferTooSmall);
0189:         }
0190: 
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 07 (line 224)

```rust
0214:             } else {
0215:                 dst[di] = src[si];
0216:                 di += 1;
0217:                 si += 1;
0218:             }
0219:         }
0220: 
0221:         Ok(di)
0222:     }
0223: 
0224:     fn name(&self) -> &'static str {
0225:         "lz4"
0226:     }
0227: }
0228: 
0229: /// ZSTD sıkıştırıcısı — daha agresif RLE + delta encoding
0230: pub struct ZstdCompressor;
0231: 
0232: impl Compressor for ZstdCompressor {
0233:     fn compress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError> {
0234:         // ZSTD daha iyi oran — Lz4 compressor'ı kullan (aynı format)
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 08 (line 230)

```rust
0220: 
0221:         Ok(di)
0222:     }
0223: 
0224:     fn name(&self) -> &'static str {
0225:         "lz4"
0226:     }
0227: }
0228: 
0229: /// ZSTD sıkıştırıcısı — daha agresif RLE + delta encoding
0230: pub struct ZstdCompressor;
0231: 
0232: impl Compressor for ZstdCompressor {
0233:     fn compress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError> {
0234:         // ZSTD daha iyi oran — Lz4 compressor'ı kullan (aynı format)
0235:         // Gerçek zstd kütüphanesi no_std'de kullanılamadığı için aynı RLE kullanılır
0236:         Lz4Compressor.compress(src, dst)
0237:     }
0238: 
0239:     fn decompress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError> {
0240:         Lz4Compressor.decompress(src, dst)
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 09 (line 233)

```rust
0223: 
0224:     fn name(&self) -> &'static str {
0225:         "lz4"
0226:     }
0227: }
0228: 
0229: /// ZSTD sıkıştırıcısı — daha agresif RLE + delta encoding
0230: pub struct ZstdCompressor;
0231: 
0232: impl Compressor for ZstdCompressor {
0233:     fn compress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError> {
0234:         // ZSTD daha iyi oran — Lz4 compressor'ı kullan (aynı format)
0235:         // Gerçek zstd kütüphanesi no_std'de kullanılamadığı için aynı RLE kullanılır
0236:         Lz4Compressor.compress(src, dst)
0237:     }
0238: 
0239:     fn decompress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError> {
0240:         Lz4Compressor.decompress(src, dst)
0241:     }
0242: 
0243:     fn name(&self) -> &'static str {
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 10 (line 239)

```rust
0229: /// ZSTD sıkıştırıcısı — daha agresif RLE + delta encoding
0230: pub struct ZstdCompressor;
0231: 
0232: impl Compressor for ZstdCompressor {
0233:     fn compress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError> {
0234:         // ZSTD daha iyi oran — Lz4 compressor'ı kullan (aynı format)
0235:         // Gerçek zstd kütüphanesi no_std'de kullanılamadığı için aynı RLE kullanılır
0236:         Lz4Compressor.compress(src, dst)
0237:     }
0238: 
0239:     fn decompress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError> {
0240:         Lz4Compressor.decompress(src, dst)
0241:     }
0242: 
0243:     fn name(&self) -> &'static str {
0244:         "zstd"
0245:     }
0246: }
0247: 
0248: // ============================================================================
0249: // ZSWAP GİRDİSİ
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 11 (line 243)

```rust
0233:     fn compress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError> {
0234:         // ZSTD daha iyi oran — Lz4 compressor'ı kullan (aynı format)
0235:         // Gerçek zstd kütüphanesi no_std'de kullanılamadığı için aynı RLE kullanılır
0236:         Lz4Compressor.compress(src, dst)
0237:     }
0238: 
0239:     fn decompress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError> {
0240:         Lz4Compressor.decompress(src, dst)
0241:     }
0242: 
0243:     fn name(&self) -> &'static str {
0244:         "zstd"
0245:     }
0246: }
0247: 
0248: // ============================================================================
0249: // ZSWAP GİRDİSİ
0250: // ============================================================================
0251: 
0252: /// Sıkıştırılmış takas girdisi
0253: #[derive(Debug)]
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 12 (line 254)

```rust
0244:         "zstd"
0245:     }
0246: }
0247: 
0248: // ============================================================================
0249: // ZSWAP GİRDİSİ
0250: // ============================================================================
0251: 
0252: /// Sıkıştırılmış takas girdisi
0253: #[derive(Debug)]
0254: pub struct ZswapEntry {
0255:     /// Özgün takas ofseti
0256:     pub swap_offset: u64,
0257:     /// Sıkıştırılmış veri tanıtıcısı (handle)
0258:     pub handle: u64,
0259:     /// Özgün boyut
0260:     pub orig_size: u32,
0261:     /// Sıkıştırılmış boyut
0262:     pub comp_size: u32,
0263:     /// Havuz kimliği
0264:     pub pool_id: u32,
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 13 (line 270)

```rust
0260:     pub orig_size: u32,
0261:     /// Sıkıştırılmış boyut
0262:     pub comp_size: u32,
0263:     /// Havuz kimliği
0264:     pub pool_id: u32,
0265:     /// Referans sayacı
0266:     pub ref_count: AtomicU32,
0267: }
0268: 
0269: impl ZswapEntry {
0270:     pub fn new(
0271:         swap_offset: u64,
0272:         handle: u64,
0273:         orig_size: u32,
0274:         comp_size: u32,
0275:         pool_id: u32,
0276:     ) -> Self {
0277:         Self {
0278:             swap_offset,
0279:             handle,
0280:             orig_size,
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 14 (line 288)

```rust
0278:             swap_offset,
0279:             handle,
0280:             orig_size,
0281:             comp_size,
0282:             pool_id,
0283:             ref_count: AtomicU32::new(1),
0284:         }
0285:     }
0286: 
0287:     /// Sıkıştırma oranını al
0288:     pub fn compression_ratio(&self) -> f32 {
0289:         if self.orig_size == 0 {
0290:             return 0.0;
0291:         }
0292:         (self.orig_size - self.comp_size) as f32 / self.orig_size as f32
0293:     }
0294: }
0295: 
0296: impl Clone for ZswapEntry {
0297:     fn clone(&self) -> Self {
0298:         Self {
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 15 (line 297)

```rust
0287:     /// Sıkıştırma oranını al
0288:     pub fn compression_ratio(&self) -> f32 {
0289:         if self.orig_size == 0 {
0290:             return 0.0;
0291:         }
0292:         (self.orig_size - self.comp_size) as f32 / self.orig_size as f32
0293:     }
0294: }
0295: 
0296: impl Clone for ZswapEntry {
0297:     fn clone(&self) -> Self {
0298:         Self {
0299:             swap_offset: self.swap_offset,
0300:             handle: self.handle,
0301:             orig_size: self.orig_size,
0302:             comp_size: self.comp_size,
0303:             pool_id: self.pool_id,
0304:             ref_count: AtomicU32::new(self.ref_count.load(Ordering::Relaxed)),
0305:         }
0306:     }
0307: }
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 16 (line 314)

```rust
0304:             ref_count: AtomicU32::new(self.ref_count.load(Ordering::Relaxed)),
0305:         }
0306:     }
0307: }
0308: 
0309: // ============================================================================
0310: // ZSWAP HAVUZU
0311: // ============================================================================
0312: 
0313: /// Zswap havuzu
0314: pub struct ZswapPool {
0315:     /// Havuz kimliği
0316:     pub id: u32,
0317:     /// Sıkıştırıcı
0318:     pub compressor: Arc<dyn Compressor>,
0319:     /// Tahsis edilen sayfalar
0320:     pub allocated_pages: AtomicU64,
0321:     /// Sıkıştırılmış sayfalar
0322:     pub compressed_pages: AtomicU64,
0323:     /// Toplam özgün boyut
0324:     pub total_orig_size: AtomicU64,
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 17 (line 332)

```rust
0322:     pub compressed_pages: AtomicU64,
0323:     /// Toplam özgün boyut
0324:     pub total_orig_size: AtomicU64,
0325:     /// Toplam sıkıştırılmış boyut
0326:     pub total_comp_size: AtomicU64,
0327:     /// Girdiler
0328:     pub entries: Mutex<BTreeMap<u64, ZswapEntry>>,
0329: }
0330: 
0331: impl ZswapPool {
0332:     pub fn new(id: u32, compressor: Arc<dyn Compressor>) -> Self {
0333:         Self {
0334:             id,
0335:             compressor,
0336:             allocated_pages: AtomicU64::new(0),
0337:             compressed_pages: AtomicU64::new(0),
0338:             total_orig_size: AtomicU64::new(0),
0339:             total_comp_size: AtomicU64::new(0),
0340:             entries: Mutex::new(BTreeMap::new()),
0341:         }
0342:     }
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 18 (line 345)

```rust
0335:             compressor,
0336:             allocated_pages: AtomicU64::new(0),
0337:             compressed_pages: AtomicU64::new(0),
0338:             total_orig_size: AtomicU64::new(0),
0339:             total_comp_size: AtomicU64::new(0),
0340:             entries: Mutex::new(BTreeMap::new()),
0341:         }
0342:     }
0343: 
0344:     /// Sayfayı sakla
0345:     pub fn store(&self, swap_offset: u64, data: &[u8]) -> Result<ZswapEntry, ZswapError> {
0346:         let page_size = 4096;
0347:         let mut compressed = vec![0u8; page_size * 2];
0348: 
0349:         // Sıkıştır
0350:         let comp_size = self.compressor.compress(data, &mut compressed)?;
0351: 
0352:         // Tanıtıcı tahsis et (zbud/zsmalloc'tan tahsis eder)
0353:         let handle = self.alloc_handle(&compressed[..comp_size])?;
0354: 
0355:         let entry = ZswapEntry::new(
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 19 (line 377)

```rust
0367:         self.total_comp_size
0368:             .fetch_add(comp_size as u64, Ordering::Relaxed);
0369: 
0370:         // Girdiyi sakla
0371:         self.entries.lock().insert(swap_offset, entry.clone());
0372: 
0373:         Ok(entry)
0374:     }
0375: 
0376:     /// Sayfayı yükle
0377:     pub fn load(&self, swap_offset: u64, data: &mut [u8]) -> Result<(), ZswapError> {
0378:         let entries = self.entries.lock();
0379:         let entry = entries.get(&swap_offset).ok_or(ZswapError::NotFound)?;
0380: 
0381:         // Sıkıştırılmış veriyi al
0382:         let compressed = self.get_data(entry.handle)?;
0383: 
0384:         // Aç
0385:         let _ = self.compressor.decompress(&compressed, data)?;
0386: 
0387:         Ok(())
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 20 (line 391)

```rust
0381:         // Sıkıştırılmış veriyi al
0382:         let compressed = self.get_data(entry.handle)?;
0383: 
0384:         // Aç
0385:         let _ = self.compressor.decompress(&compressed, data)?;
0386: 
0387:         Ok(())
0388:     }
0389: 
0390:     /// Sayfayı kaldır
0391:     pub fn remove(&self, swap_offset: u64) -> bool {
0392:         if let Some(entry) = self.entries.lock().remove(&swap_offset) {
0393:             self.free_handle(entry.handle);
0394: 
0395:             self.compressed_pages.fetch_sub(1, Ordering::Relaxed);
0396:             self.total_orig_size
0397:                 .fetch_sub(entry.orig_size as u64, Ordering::Relaxed);
0398:             self.total_comp_size
0399:                 .fetch_sub(entry.comp_size as u64, Ordering::Relaxed);
0400: 
0401:             return true;
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

---

## src/posix/io_uring_ring.rs

- Satir sayisi: 859
- Derin kesit sayisi: 20

### Kesit 01 (line 47)

```rust
0037: 
0038: use core::cell::UnsafeCell;
0039: use core::sync::atomic::{AtomicU32, Ordering};
0040: 
0041: use super::{copy_from_user, validate_user_range, write_user_bytes};
0042: 
0043: /// Send-safe raw pointer wrapper.
0044: /// CompletionRing tüm alanları atomik olduğu için
0045: /// farklı thread'lerden erişim güvenlidir.
0046: #[derive(Clone, Copy)]
0047: pub struct SendPtr<T>(*const T);
0048: unsafe impl<T> Send for SendPtr<T> {}
0049: unsafe impl<T> Sync for SendPtr<T> {}
0050: 
0051: impl<T> SendPtr<T> {
0052:     /// Raw pointer'dan SendPtr oluşturur.
0053:     pub fn new(ptr: *const T) -> Self {
0054:         Self(ptr)
0055:     }
0056:     /// İç pointer'a erişim sağlar.
0057:     pub fn as_ptr(&self) -> *const T {
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 02 (line 53)

```rust
0043: /// Send-safe raw pointer wrapper.
0044: /// CompletionRing tüm alanları atomik olduğu için
0045: /// farklı thread'lerden erişim güvenlidir.
0046: #[derive(Clone, Copy)]
0047: pub struct SendPtr<T>(*const T);
0048: unsafe impl<T> Send for SendPtr<T> {}
0049: unsafe impl<T> Sync for SendPtr<T> {}
0050: 
0051: impl<T> SendPtr<T> {
0052:     /// Raw pointer'dan SendPtr oluşturur.
0053:     pub fn new(ptr: *const T) -> Self {
0054:         Self(ptr)
0055:     }
0056:     /// İç pointer'a erişim sağlar.
0057:     pub fn as_ptr(&self) -> *const T {
0058:         self.0
0059:     }
0060: }
0061: 
0062: /// Ring buffer kapasitesi — 2'nin kuvveti OLMALI (mask için).
0063: /// Linux varsayılanı genellikle 128 veya 256'dır.
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 03 (line 57)

```rust
0047: pub struct SendPtr<T>(*const T);
0048: unsafe impl<T> Send for SendPtr<T> {}
0049: unsafe impl<T> Sync for SendPtr<T> {}
0050: 
0051: impl<T> SendPtr<T> {
0052:     /// Raw pointer'dan SendPtr oluşturur.
0053:     pub fn new(ptr: *const T) -> Self {
0054:         Self(ptr)
0055:     }
0056:     /// İç pointer'a erişim sağlar.
0057:     pub fn as_ptr(&self) -> *const T {
0058:         self.0
0059:     }
0060: }
0061: 
0062: /// Ring buffer kapasitesi — 2'nin kuvveti OLMALI (mask için).
0063: /// Linux varsayılanı genellikle 128 veya 256'dır.
0064: const RING_SIZE: usize = 256;
0065: 
0066: /// Ring mask = RING_SIZE - 1 (bit maskeleme ile modülo yerine AND kullanarak hız kazanılır)
0067: const RING_MASK: u32 = (RING_SIZE - 1) as u32;
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 04 (line 79)

```rust
0069: // ============================================================================
0070: // SQE (Submission Queue Entry) — Kullanıcı → Kernel yönlü
0071: // ============================================================================
0072: 
0073: /// io_uring Submission Queue Entry.
0074: ///
0075: /// Kullanıcı alanı tarafından yazılır, kernel tarafından okunur.
0076: /// Linux `struct io_uring_sqe` ile ABI uyumludur.
0077: #[repr(C)]
0078: #[derive(Clone, Copy)]
0079: pub struct RingSqe {
0080:     /// İşlem kodu: IORING_OP_NOP(0), IORING_OP_READV(1), IORING_OP_WRITEV(2) vb.
0081:     pub opcode: u8,
0082:     /// SQE bayrakları: IOSQE_FIXED_FILE, IOSQE_IO_DRAIN, IOSQE_IO_LINK vb.
0083:     pub flags: u8,
0084:     /// I/O önceliği (ionice seviyesi)
0085:     pub ioprio: u16,
0086:     /// Hedef dosya tanımlayıcı
0087:     pub fd: i32,
0088:     /// Dosya ofseti (okuma/yazma başlangıç noktası)
0089:     pub off: u64,
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 05 (line 109)

```rust
0099:     pub buf_index: u16,
0100:     /// Personality indeksi (credential yönetimi)
0101:     pub personality: u16,
0102:     /// Splice/tee işlemleri için kaynak FD
0103:     pub splice_fd_in: i32,
0104:     /// Gelecek kullanım için yedek alan
0105:     pub _pad: [u64; 2],
0106: }
0107: 
0108: impl Default for RingSqe {
0109:     fn default() -> Self {
0110:         Self {
0111:             opcode: 0,
0112:             flags: 0,
0113:             ioprio: 0,
0114:             fd: -1,
0115:             off: 0,
0116:             addr: 0,
0117:             len: 0,
0118:             rw_flags: 0,
0119:             user_data: 0,
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 06 (line 134)

```rust
0124:         }
0125:     }
0126: }
0127: 
0128: /// io_uring Completion Queue Entry.
0129: ///
0130: /// Kernel tarafından yazılır, kullanıcı alanı tarafından okunur.
0131: /// Linux `struct io_uring_cqe` ile ABI uyumludur.
0132: #[repr(C)]
0133: #[derive(Clone, Copy)]
0134: pub struct RingCqe {
0135:     /// SQE'den birebir kopyalanan kullanıcı tanımlı veri
0136:     pub user_data: u64,
0137:     /// İşlem sonucu: >=0 başarı, <0 hata kodu (errno)
0138:     pub res: i32,
0139:     /// CQE bayrakları: IORING_CQE_F_BUFFER, IORING_CQE_F_MORE vb.
0140:     pub flags: u32,
0141: }
0142: 
0143: impl Default for RingCqe {
0144:     fn default() -> Self {
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 07 (line 144)

```rust
0134: pub struct RingCqe {
0135:     /// SQE'den birebir kopyalanan kullanıcı tanımlı veri
0136:     pub user_data: u64,
0137:     /// İşlem sonucu: >=0 başarı, <0 hata kodu (errno)
0138:     pub res: i32,
0139:     /// CQE bayrakları: IORING_CQE_F_BUFFER, IORING_CQE_F_MORE vb.
0140:     pub flags: u32,
0141: }
0142: 
0143: impl Default for RingCqe {
0144:     fn default() -> Self {
0145:         Self {
0146:             user_data: 0,
0147:             res: 0,
0148:             flags: 0,
0149:         }
0150:     }
0151: }
0152: 
0153: // ============================================================================
0154: // io_uring Opcodes (Linux ABI uyumlu)
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 08 (line 195)

```rust
0185: /// Consumer: Kernel (head'den okur)
0186: ///
0187: /// ```text
0188: ///   head                              tail
0189: ///    ↓                                  ↓
0190: ///  ┌─────┬─────┬─────┬─────┬─────┬─────┬─────┐
0191: ///  │     │ SQE │ SQE │ SQE │ SQE │     │     │
0192: ///  └─────┴─────┴─────┴─────┴─────┴─────┴─────┘
0193: ///         ←── okunmamış girişler ──→
0194: /// ```
0195: pub struct SubmissionRing {
0196:     /// Kernel tarafından ilerletilir: bir sonraki okunacak SQE indeksi
0197:     head: AtomicU32,
0198:     /// Kullanıcı tarafından ilerletilir: bir sonraki yazılacak SQE indeksi
0199:     tail: AtomicU32,
0200:     /// SQE veri dizisi — UnsafeCell ile interior mutability (atomik koruma altında)
0201:     entries: UnsafeCell<[RingSqe; RING_SIZE]>,
0202:     /// Kuyrukta düşürülen (overflow) girişlerin sayısı
0203:     dropped: AtomicU32,
0204: }
0205: 
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 09 (line 215)

```rust
0205: 
0206: // SAFETY: SubmissionRing tüm erişimi atomic head/tail + memory barrier ile koruyor.
0207: // Aynı slot'a eşzamanlı okuma/yazma önlenir (producer tail'i artırana kadar consumer görmez).
0208: unsafe impl Send for SubmissionRing {}
0209: unsafe impl Sync for SubmissionRing {}
0210: 
0211: /// Completion Queue — Lock-Free Ring Buffer
0212: ///
0213: /// Producer: Kernel (tail'e yazar)
0214: /// Consumer: Kullanıcı alanı (head'den okur)
0215: pub struct CompletionRing {
0216:     /// Kullanıcı tarafından ilerletilir: bir sonraki okunacak CQE indeksi
0217:     head: AtomicU32,
0218:     /// Kernel tarafından ilerletilir: bir sonraki yazılacak CQE indeksi
0219:     tail: AtomicU32,
0220:     /// CQE veri dizisi — UnsafeCell ile interior mutability (atomik koruma altında)
0221:     entries: UnsafeCell<[RingCqe; RING_SIZE]>,
0222:     /// Taşma sayacı: CQ doluyken kayıp CQE sayısı
0223:     overflow: AtomicU32,
0224: }
0225: 
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 10 (line 261)

```rust
0251:                 }; RING_SIZE],
0252:             ),
0253:             dropped: AtomicU32::new(0),
0254:         }
0255:     }
0256: 
0257:     /// Ring'deki bekleyen (okunmamış) SQE sayısını döner.
0258:     ///
0259:     /// `count = tail - head` (wrapping aritmetik ile güvenli)
0260:     #[inline]
0261:     pub fn pending_count(&self) -> u32 {
0262:         let tail = self.tail.load(Ordering::Acquire);
0263:         let head = self.head.load(Ordering::Acquire);
0264:         tail.wrapping_sub(head)
0265:     }
0266: 
0267:     /// Ring'in dolu olup olmadığını kontrol eder.
0268:     #[inline]
0269:     pub fn is_full(&self) -> bool {
0270:         self.pending_count() >= RING_SIZE as u32
0271:     }
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 11 (line 269)

```rust
0259:     /// `count = tail - head` (wrapping aritmetik ile güvenli)
0260:     #[inline]
0261:     pub fn pending_count(&self) -> u32 {
0262:         let tail = self.tail.load(Ordering::Acquire);
0263:         let head = self.head.load(Ordering::Acquire);
0264:         tail.wrapping_sub(head)
0265:     }
0266: 
0267:     /// Ring'in dolu olup olmadığını kontrol eder.
0268:     #[inline]
0269:     pub fn is_full(&self) -> bool {
0270:         self.pending_count() >= RING_SIZE as u32
0271:     }
0272: 
0273:     /// Ring'in boş olup olmadığını kontrol eder.
0274:     #[inline]
0275:     pub fn is_empty(&self) -> bool {
0276:         self.pending_count() == 0
0277:     }
0278: 
0279:     /// Ring kapasitesini döner.
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 12 (line 275)

```rust
0265:     }
0266: 
0267:     /// Ring'in dolu olup olmadığını kontrol eder.
0268:     #[inline]
0269:     pub fn is_full(&self) -> bool {
0270:         self.pending_count() >= RING_SIZE as u32
0271:     }
0272: 
0273:     /// Ring'in boş olup olmadığını kontrol eder.
0274:     #[inline]
0275:     pub fn is_empty(&self) -> bool {
0276:         self.pending_count() == 0
0277:     }
0278: 
0279:     /// Ring kapasitesini döner.
0280:     #[inline]
0281:     pub fn capacity(&self) -> u32 {
0282:         RING_SIZE as u32
0283:     }
0284: 
0285:     /// PRODUCER (Kullanıcı Alanı): Yeni bir SQE ekler.
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 13 (line 281)

```rust
0271:     }
0272: 
0273:     /// Ring'in boş olup olmadığını kontrol eder.
0274:     #[inline]
0275:     pub fn is_empty(&self) -> bool {
0276:         self.pending_count() == 0
0277:     }
0278: 
0279:     /// Ring kapasitesini döner.
0280:     #[inline]
0281:     pub fn capacity(&self) -> u32 {
0282:         RING_SIZE as u32
0283:     }
0284: 
0285:     /// PRODUCER (Kullanıcı Alanı): Yeni bir SQE ekler.
0286:     ///
0287:     /// ## Sıralama Garantisi
0288:     /// 1. SQE verisi yazılır (entries dizisine)
0289:     /// 2. `smp_wmb()` — yazma bariyeri (sfence)
0290:     /// 3. `tail` atomik olarak artırılır
0291:     ///
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 14 (line 298)

```rust
0288:     /// 1. SQE verisi yazılır (entries dizisine)
0289:     /// 2. `smp_wmb()` — yazma bariyeri (sfence)
0290:     /// 3. `tail` atomik olarak artırılır
0291:     ///
0292:     /// Bu sıralama, kernel'ın tail'i okuduğunda SQE verisinin
0293:     /// kesinlikle görünür olmasını garanti eder.
0294:     ///
0295:     /// ## Dönüş
0296:     /// - `Ok(index)`: Eklenen SQE'nin ring indeksi
0297:     /// - `Err(())`: Ring dolu (EAGAIN)
0298:     pub fn push(&self, sqe: RingSqe) -> Result<u32, ()> {
0299:         let tail = self.tail.load(Ordering::Relaxed);
0300:         let head = self.head.load(Ordering::Acquire);
0301: 
0302:         // Ring dolu mu?
0303:         if tail.wrapping_sub(head) >= RING_SIZE as u32 {
0304:             self.dropped.fetch_add(1, Ordering::Relaxed);
0305:             return Err(());
0306:         }
0307: 
0308:         let index = (tail & RING_MASK) as usize;
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 15 (line 338)

```rust
0328:     /// CONSUMER (Kernel): Bir sonraki SQE'yi okur ve döner.
0329:     ///
0330:     /// ## Sıralama Garantisi
0331:     /// 1. `tail` atomik olarak okunur (Acquire)
0332:     /// 2. `smp_rmb()` — okuma bariyeri (lfence)
0333:     /// 3. SQE verisi okunur
0334:     /// 4. `head` atomik olarak artırılır (Release)
0335:     ///
0336:     /// Bu sıralama, okunan SQE verisinin kesinlikle güncel
0337:     /// olmasını garanti eder.
0338:     pub fn pop(&self) -> Option<RingSqe> {
0339:         let head = self.head.load(Ordering::Relaxed);
0340:         let tail = self.tail.load(Ordering::Acquire);
0341: 
0342:         // Ring boş mu?
0343:         if head == tail {
0344:             return None;
0345:         }
0346: 
0347:         // 1. Okuma bariyeri: tail okunduktan sonra veri oku
0348:         crate::memory_barriers::smp_rmb();
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 16 (line 371)

```rust
0361:         Some(sqe)
0362:     }
0363: 
0364:     /// Kernel: Birden fazla SQE'yi toplu olarak (batch) okur.
0365:     ///
0366:     /// Toplu okuma, bariyer maliyetini amortisman eder:
0367:     /// - Tek smp_rmb() çağrısı ile N adet SQE okunur
0368:     /// - Head yalnızca bir kez güncellenir
0369:     ///
0370:     /// `max_count` kadar veya mevcut olanlar kadar SQE okur.
0371:     pub fn pop_batch(&self, out: &mut [RingSqe], max_count: usize) -> usize {
0372:         let head = self.head.load(Ordering::Relaxed);
0373:         let tail = self.tail.load(Ordering::Acquire);
0374: 
0375:         let available = tail.wrapping_sub(head) as usize;
0376:         if available == 0 {
0377:             return 0;
0378:         }
0379: 
0380:         let count = available.min(max_count).min(out.len());
0381: 
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 17 (line 420)

```rust
0410:                     res: 0,
0411:                     flags: 0,
0412:                 }; RING_SIZE],
0413:             ),
0414:             overflow: AtomicU32::new(0),
0415:         }
0416:     }
0417: 
0418:     /// Ring'deki tamamlanmış (okunmamış) CQE sayısını döner.
0419:     #[inline]
0420:     pub fn pending_count(&self) -> u32 {
0421:         let tail = self.tail.load(Ordering::Acquire);
0422:         let head = self.head.load(Ordering::Acquire);
0423:         tail.wrapping_sub(head)
0424:     }
0425: 
0426:     /// Ring'in dolu olup olmadığını kontrol eder.
0427:     #[inline]
0428:     pub fn is_full(&self) -> bool {
0429:         self.pending_count() >= RING_SIZE as u32
0430:     }
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 18 (line 428)

```rust
0418:     /// Ring'deki tamamlanmış (okunmamış) CQE sayısını döner.
0419:     #[inline]
0420:     pub fn pending_count(&self) -> u32 {
0421:         let tail = self.tail.load(Ordering::Acquire);
0422:         let head = self.head.load(Ordering::Acquire);
0423:         tail.wrapping_sub(head)
0424:     }
0425: 
0426:     /// Ring'in dolu olup olmadığını kontrol eder.
0427:     #[inline]
0428:     pub fn is_full(&self) -> bool {
0429:         self.pending_count() >= RING_SIZE as u32
0430:     }
0431: 
0432:     /// Ring'in boş olup olmadığını kontrol eder.
0433:     #[inline]
0434:     pub fn is_empty(&self) -> bool {
0435:         self.pending_count() == 0
0436:     }
0437: 
0438:     /// PRODUCER (Kernel): Tamamlanan bir işlemin CQE'sini ring'e ekler.
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 19 (line 434)

```rust
0424:     }
0425: 
0426:     /// Ring'in dolu olup olmadığını kontrol eder.
0427:     #[inline]
0428:     pub fn is_full(&self) -> bool {
0429:         self.pending_count() >= RING_SIZE as u32
0430:     }
0431: 
0432:     /// Ring'in boş olup olmadığını kontrol eder.
0433:     #[inline]
0434:     pub fn is_empty(&self) -> bool {
0435:         self.pending_count() == 0
0436:     }
0437: 
0438:     /// PRODUCER (Kernel): Tamamlanan bir işlemin CQE'sini ring'e ekler.
0439:     ///
0440:     /// ## Sıralama Garantisi
0441:     /// 1. CQE verisi yazılır (entries dizisine)
0442:     /// 2. `smp_wmb()` — yazma bariyeri (sfence)
0443:     /// 3. `tail` atomik olarak artırılır (Release)
0444:     ///
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 20 (line 447)

```rust
0437: 
0438:     /// PRODUCER (Kernel): Tamamlanan bir işlemin CQE'sini ring'e ekler.
0439:     ///
0440:     /// ## Sıralama Garantisi
0441:     /// 1. CQE verisi yazılır (entries dizisine)
0442:     /// 2. `smp_wmb()` — yazma bariyeri (sfence)
0443:     /// 3. `tail` atomik olarak artırılır (Release)
0444:     ///
0445:     /// Bu sıralama, kullanıcının tail'i okuduğunda CQE verisinin
0446:     /// kesinlikle görünür olmasını garanti eder.
0447:     pub fn push(&self, user_data: u64, res: i32, flags: u32) -> Result<(), ()> {
0448:         let tail = self.tail.load(Ordering::Relaxed);
0449:         let head = self.head.load(Ordering::Acquire);
0450: 
0451:         // Ring dolu mu?
0452:         if tail.wrapping_sub(head) >= RING_SIZE as u32 {
0453:             self.overflow.fetch_add(1, Ordering::Relaxed);
0454:             return Err(());
0455:         }
0456: 
0457:         let index = (tail & RING_MASK) as usize;
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

---

## src/net/tls.rs

- Satir sayisi: 4111
- Derin kesit sayisi: 20

### Kesit 01 (line 107)

```rust
0097: pub const TLS_VERSION_1_3: u16 = 0x0303;
0098: 
0099: /// TLS 1.3 kayıt türleri (record layer content type)
0100: ///
0101: /// Her TLS kaydının ilk byte'ı içerik türünü belirtir:
0102: /// - 20: ChangeCipherSpec (geriye uyumluluk, TLS 1.3'te anlamsız)
0103: /// - 21: Alert (uyarı/hata mesajları)
0104: /// - 22: Handshake (el sıkışma mesajları)
0105: /// - 23: ApplicationData (şifreli uygulama verisi)
0106: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0107: pub enum ContentType {
0108:     ChangeCipherSpec = 20,
0109:     Alert = 21,
0110:     Handshake = 22,
0111:     ApplicationData = 23,
0112: }
0113: 
0114: impl ContentType {
0115:     pub fn from_u8(v: u8) -> Option<Self> {
0116:         match v {
0117:             20 => Some(ContentType::ChangeCipherSpec),
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 02 (line 115)

```rust
0105: /// - 23: ApplicationData (şifreli uygulama verisi)
0106: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0107: pub enum ContentType {
0108:     ChangeCipherSpec = 20,
0109:     Alert = 21,
0110:     Handshake = 22,
0111:     ApplicationData = 23,
0112: }
0113: 
0114: impl ContentType {
0115:     pub fn from_u8(v: u8) -> Option<Self> {
0116:         match v {
0117:             20 => Some(ContentType::ChangeCipherSpec),
0118:             21 => Some(ContentType::Alert),
0119:             22 => Some(ContentType::Handshake),
0120:             23 => Some(ContentType::ApplicationData),
0121:             _ => None,
0122:         }
0123:     }
0124: }
0125: 
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 03 (line 131)

```rust
0121:             _ => None,
0122:         }
0123:     }
0124: }
0125: 
0126: /// TLS 1.3 el sıkışma mesaj türleri
0127: ///
0128: /// El sıkışma akışı (tam): ClientHello -> ServerHello ->
0129: /// EncryptedExtensions -> Certificate -> CertificateVerify -> Finished
0130: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0131: pub enum HandshakeType {
0132:     ClientHello = 1,
0133:     ServerHello = 2,
0134:     NewSessionTicket = 4,
0135:     EndOfEarlyData = 5,
0136:     EncryptedExtensions = 8,
0137:     Certificate = 11,
0138:     CertificateRequest = 13,
0139:     CertificateVerify = 15,
0140:     Finished = 20,
0141:     KeyUpdate = 24,
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 04 (line 145)

```rust
0135:     EndOfEarlyData = 5,
0136:     EncryptedExtensions = 8,
0137:     Certificate = 11,
0138:     CertificateRequest = 13,
0139:     CertificateVerify = 15,
0140:     Finished = 20,
0141:     KeyUpdate = 24,
0142: }
0143: 
0144: impl HandshakeType {
0145:     pub fn from_u8(v: u8) -> Option<Self> {
0146:         match v {
0147:             1 => Some(HandshakeType::ClientHello),
0148:             2 => Some(HandshakeType::ServerHello),
0149:             4 => Some(HandshakeType::NewSessionTicket),
0150:             5 => Some(HandshakeType::EndOfEarlyData),
0151:             8 => Some(HandshakeType::EncryptedExtensions),
0152:             11 => Some(HandshakeType::Certificate),
0153:             13 => Some(HandshakeType::CertificateRequest),
0154:             15 => Some(HandshakeType::CertificateVerify),
0155:             20 => Some(HandshakeType::Finished),
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 05 (line 167)

```rust
0157:             _ => None,
0158:         }
0159:     }
0160: }
0161: 
0162: /// TLS 1.3 şifre paketleri
0163: ///
0164: /// TLS 1.3'te yalnızca AEAD (Authenticated Encryption with Associated Data)
0165: /// şifreleme algoritmaları desteklenir. Her paket: şifreleme + hash içerir.
0166: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0167: pub enum CipherSuite {
0168:     Aes128GcmSha256 = 0x1301,
0169:     Aes256GcmSha384 = 0x1302,
0170:     ChaCha20Poly1305Sha256 = 0x1303,
0171: }
0172: 
0173: impl CipherSuite {
0174:     pub fn from_u16(v: u16) -> Option<Self> {
0175:         match v {
0176:             0x1301 => Some(CipherSuite::Aes128GcmSha256),
0177:             0x1302 => Some(CipherSuite::Aes256GcmSha384),
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 06 (line 174)

```rust
0164: /// TLS 1.3'te yalnızca AEAD (Authenticated Encryption with Associated Data)
0165: /// şifreleme algoritmaları desteklenir. Her paket: şifreleme + hash içerir.
0166: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0167: pub enum CipherSuite {
0168:     Aes128GcmSha256 = 0x1301,
0169:     Aes256GcmSha384 = 0x1302,
0170:     ChaCha20Poly1305Sha256 = 0x1303,
0171: }
0172: 
0173: impl CipherSuite {
0174:     pub fn from_u16(v: u16) -> Option<Self> {
0175:         match v {
0176:             0x1301 => Some(CipherSuite::Aes128GcmSha256),
0177:             0x1302 => Some(CipherSuite::Aes256GcmSha384),
0178:             0x1303 => Some(CipherSuite::ChaCha20Poly1305Sha256),
0179:             _ => None,
0180:         }
0181:     }
0182: 
0183:     pub fn key_len(&self) -> usize {
0184:         match self {
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 07 (line 183)

```rust
0173: impl CipherSuite {
0174:     pub fn from_u16(v: u16) -> Option<Self> {
0175:         match v {
0176:             0x1301 => Some(CipherSuite::Aes128GcmSha256),
0177:             0x1302 => Some(CipherSuite::Aes256GcmSha384),
0178:             0x1303 => Some(CipherSuite::ChaCha20Poly1305Sha256),
0179:             _ => None,
0180:         }
0181:     }
0182: 
0183:     pub fn key_len(&self) -> usize {
0184:         match self {
0185:             CipherSuite::Aes128GcmSha256 => 16,
0186:             CipherSuite::Aes256GcmSha384 => 32,
0187:             CipherSuite::ChaCha20Poly1305Sha256 => 32,
0188:         }
0189:     }
0190: 
0191:     pub fn iv_len(&self) -> usize {
0192:         12
0193:     }
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 08 (line 191)

```rust
0181:     }
0182: 
0183:     pub fn key_len(&self) -> usize {
0184:         match self {
0185:             CipherSuite::Aes128GcmSha256 => 16,
0186:             CipherSuite::Aes256GcmSha384 => 32,
0187:             CipherSuite::ChaCha20Poly1305Sha256 => 32,
0188:         }
0189:     }
0190: 
0191:     pub fn iv_len(&self) -> usize {
0192:         12
0193:     }
0194: }
0195: 
0196: /// TLS 1.3 named groups
0197: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0198: pub enum NamedGroup {
0199:     Secp256r1 = 0x0017,
0200:     Secp384r1 = 0x0018,
0201:     Secp521r1 = 0x0019,
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 09 (line 198)

```rust
0188:         }
0189:     }
0190: 
0191:     pub fn iv_len(&self) -> usize {
0192:         12
0193:     }
0194: }
0195: 
0196: /// TLS 1.3 named groups
0197: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0198: pub enum NamedGroup {
0199:     Secp256r1 = 0x0017,
0200:     Secp384r1 = 0x0018,
0201:     Secp521r1 = 0x0019,
0202:     X25519 = 0x001D,
0203:     X448 = 0x001E,
0204: }
0205: 
0206: impl NamedGroup {
0207:     pub fn from_u16(v: u16) -> Option<Self> {
0208:         match v {
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 10 (line 207)

```rust
0197: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0198: pub enum NamedGroup {
0199:     Secp256r1 = 0x0017,
0200:     Secp384r1 = 0x0018,
0201:     Secp521r1 = 0x0019,
0202:     X25519 = 0x001D,
0203:     X448 = 0x001E,
0204: }
0205: 
0206: impl NamedGroup {
0207:     pub fn from_u16(v: u16) -> Option<Self> {
0208:         match v {
0209:             0x0017 => Some(NamedGroup::Secp256r1),
0210:             0x0018 => Some(NamedGroup::Secp384r1),
0211:             0x0019 => Some(NamedGroup::Secp521r1),
0212:             0x001D => Some(NamedGroup::X25519),
0213:             0x001E => Some(NamedGroup::X448),
0214:             _ => None,
0215:         }
0216:     }
0217: }
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 11 (line 224)

```rust
0214:             _ => None,
0215:         }
0216:     }
0217: }
0218: 
0219: /// TLS 1.3 imza şemaları (sertifika doğrulama için)
0220: ///
0221: /// TLS 1.3'te RSA-PKCS1 v1.5 yalnızca geriye uyumluluk için tutulmuştur.
0222: /// Önerilen: ECDSA veya RSA-PSS kullanımı.
0223: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0224: pub enum SignatureScheme {
0225:     RsaPkcs1Sha256 = 0x0401,
0226:     RsaPkcs1Sha384 = 0x0402,
0227:     RsaPkcs1Sha512 = 0x0403,
0228:     EcdsaSecp256r1Sha256 = 0x0404,
0229:     EcdsaSecp384r1Sha384 = 0x0503,
0230:     EcdsaSecp521r1Sha512 = 0x0603,
0231:     RsaPssRsaeSha256 = 0x0804,
0232:     RsaPssRsaeSha384 = 0x0805,
0233:     RsaPssRsaeSha512 = 0x0806,
0234:     Ed25519 = 0x0807,
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 12 (line 243)

```rust
0233:     RsaPssRsaeSha512 = 0x0806,
0234:     Ed25519 = 0x0807,
0235: }
0236: 
0237: // ============================================================================
0238: // TLS HATA TİPLERİ
0239: // ============================================================================
0240: 
0241: /// TLS hata türleri
0242: #[derive(Clone, Debug, PartialEq, Eq)]
0243: pub enum TlsError {
0244:     InvalidState,
0245:     InvalidMessage,
0246:     KeyExchangeFailed,
0247:     DecryptionFailed,
0248:     EncryptionFailed,
0249:     CertificateVerificationFailed,
0250:     InvalidCertificate,
0251:     Timeout,
0252:     ConnectionClosed,
0253:     Alert(AlertLevel, AlertDescription),
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 13 (line 259)

```rust
0249:     CertificateVerificationFailed,
0250:     InvalidCertificate,
0251:     Timeout,
0252:     ConnectionClosed,
0253:     Alert(AlertLevel, AlertDescription),
0254:     InternalError,
0255: }
0256: 
0257: /// TLS alert levels
0258: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0259: pub enum AlertLevel {
0260:     Warning = 1,
0261:     Fatal = 2,
0262: }
0263: 
0264: /// TLS alert descriptions
0265: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0266: pub enum AlertDescription {
0267:     CloseNotify = 0,
0268:     UnexpectedMessage = 10,
0269:     BadRecordMac = 20,
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 14 (line 266)

```rust
0256: 
0257: /// TLS alert levels
0258: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0259: pub enum AlertLevel {
0260:     Warning = 1,
0261:     Fatal = 2,
0262: }
0263: 
0264: /// TLS alert descriptions
0265: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0266: pub enum AlertDescription {
0267:     CloseNotify = 0,
0268:     UnexpectedMessage = 10,
0269:     BadRecordMac = 20,
0270:     HandshakeFailure = 40,
0271:     BadCertificate = 42,
0272:     CertificateExpired = 45,
0273:     IllegalParameter = 47,
0274:     InternalError = 80,
0275: }
0276: 
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 15 (line 295)

```rust
0285: //     -> ServerHelloReceived  (ServerHello alındı, ECDHE tamamlandı)
0286: //     -> EncryptedExtensionsReceived
0287: //     -> CertificateReceived
0288: //     -> CertificateVerifyReceived  (imza doğrulandı)
0289: //     -> FinishedReceived      (sunucu Finished alındı)
0290: //     -> Established           (bağlantı hazır, uygulama verisi)
0291: //     -> Closed / Error
0292: 
0293: /// TLS el sıkışma durum makinesi
0294: #[derive(Clone, Debug, PartialEq, Eq)]
0295: pub enum TlsState {
0296:     Initial,
0297:     ClientHelloSent,
0298:     ServerHelloReceived,
0299:     EncryptedExtensionsReceived,
0300:     CertificateReceived,
0301:     CertificateVerifyReceived,
0302:     FinishedReceived,
0303:     Established,
0304:     Closed,
0305: }
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 16 (line 324)

```rust
0314: //   +----------+---------+----------+
0315: //   | Type (1) | Ver (2) | Len (2)  |  <- 5 byte başlık
0316: //   +----------+---------+----------+
0317: //   | Veri (len byte)                |  <- Şifreli ya da açık veri
0318: //   +--------------------------------+
0319: //
0320: // TLS 1.3'te ApplicationData kayıtlarının gerçek tipi iç ContentType'ta gizlidir.
0321: 
0322: /// TLS kayıt başlığı (5 byte)
0323: #[derive(Clone, Debug)]
0324: pub struct TlsRecordHeader {
0325:     pub content_type: ContentType,
0326:     pub version: u16,
0327:     pub length: u16,
0328: }
0329: 
0330: impl TlsRecordHeader {
0331:     pub const SIZE: usize = 5;
0332: 
0333:     pub fn parse(data: &[u8]) -> Result<Self, TlsError> {
0334:         if data.len() < Self::SIZE {
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 17 (line 333)

```rust
0323: #[derive(Clone, Debug)]
0324: pub struct TlsRecordHeader {
0325:     pub content_type: ContentType,
0326:     pub version: u16,
0327:     pub length: u16,
0328: }
0329: 
0330: impl TlsRecordHeader {
0331:     pub const SIZE: usize = 5;
0332: 
0333:     pub fn parse(data: &[u8]) -> Result<Self, TlsError> {
0334:         if data.len() < Self::SIZE {
0335:             return Err(TlsError::InvalidMessage);
0336:         }
0337: 
0338:         let content_type = ContentType::from_u8(data[0]).ok_or(TlsError::InvalidMessage)?;
0339:         let version = u16::from_be_bytes([data[1], data[2]]);
0340:         let length = u16::from_be_bytes([data[3], data[4]]);
0341: 
0342:         Ok(Self {
0343:             content_type,
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 18 (line 349)

```rust
0339:         let version = u16::from_be_bytes([data[1], data[2]]);
0340:         let length = u16::from_be_bytes([data[3], data[4]]);
0341: 
0342:         Ok(Self {
0343:             content_type,
0344:             version,
0345:             length,
0346:         })
0347:     }
0348: 
0349:     pub fn to_bytes(&self) -> [u8; Self::SIZE] {
0350:         let mut buf = [0u8; Self::SIZE];
0351:         buf[0] = self.content_type as u8;
0352:         buf[1..3].copy_from_slice(&self.version.to_be_bytes());
0353:         buf[3..5].copy_from_slice(&self.length.to_be_bytes());
0354:         buf
0355:     }
0356: }
0357: 
0358: // ============================================================================
0359: // TLS EL SIKIŞTIRMA MESAJ BAŞLIĞI (Handshake Message Header)
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 19 (line 374)

```rust
0364: //
0365: // Uzunluk 3-byte big-endian olarak kodlanır (tek byte yeterli olmaz).
0366: // TlsRecord içinde taşınır: ContentType=Handshake(22) olan kayıtlarda.
0367: 
0368: /// TLS el sıkışma mesaj başlığı (4 byte)
0369: ///
0370: /// Her el sıkışma mesajının başına eklenen tip ve uzunluk bilgisi.
0371: /// MsgType: HandshakeType enum değerinden türetilir.
0372: /// Length: Gövdenin byte cinsinden uzunluğu (3-byte big-endian).
0373: #[derive(Clone, Debug)]
0374: pub struct HandshakeHeader {
0375:     pub msg_type: HandshakeType,
0376:     pub length: u32,
0377: }
0378: 
0379: impl HandshakeHeader {
0380:     pub const SIZE: usize = 4;
0381: 
0382:     pub fn parse(data: &[u8]) -> Result<Self, TlsError> {
0383:         if data.len() < Self::SIZE {
0384:             return Err(TlsError::InvalidMessage);
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 20 (line 382)

```rust
0372: /// Length: Gövdenin byte cinsinden uzunluğu (3-byte big-endian).
0373: #[derive(Clone, Debug)]
0374: pub struct HandshakeHeader {
0375:     pub msg_type: HandshakeType,
0376:     pub length: u32,
0377: }
0378: 
0379: impl HandshakeHeader {
0380:     pub const SIZE: usize = 4;
0381: 
0382:     pub fn parse(data: &[u8]) -> Result<Self, TlsError> {
0383:         if data.len() < Self::SIZE {
0384:             return Err(TlsError::InvalidMessage);
0385:         }
0386: 
0387:         let msg_type = HandshakeType::from_u8(data[0]).ok_or(TlsError::InvalidMessage)?;
0388:         let length = ((data[1] as u32) << 16) | ((data[2] as u32) << 8) | (data[3] as u32);
0389: 
0390:         Ok(Self { msg_type, length })
0391:     }
0392: 
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

---

## src/net/quic.rs

- Satir sayisi: 2311
- Derin kesit sayisi: 20

### Kesit 01 (line 109)

```rust
0099: // ============================================================================
0100: 
0101: /// QUIC sürüm 1 (RFC 9000). Paket başlığında version alanına yazılır.
0102: /// Sürüm müzakeresi (Version Negotiation) için 0x00000000 kullanılır.
0103: pub const QUIC_VERSION_1: u32 = 0x00000001;
0104: const MAX_ACK_RANGES: u64 = 256;
0105: 
0106: /// QUIC paket tipleri (uzun başlık için).
0107: /// Her tip farklı bağlantı kurulumu aşamasına karşılık gelir.
0108: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0109: pub enum QuicPacketType {
0110:     /// El sıkışmanın ilk paketi. TLS ClientHello burada taşınır.
0111:     Initial = 0x00,
0112:     /// 0-RTT verisi: önceki oturumun TLS bilgileriyle şifreli veri.
0113:     ZeroRTT = 0x01,
0114:     /// TLS el sıkışmasının devam paketi (Handshake aşaması).
0115:     Handshake = 0x02,
0116:     /// Sunucu yeniden deneme paketi: token gönderir (DDoS koruması).
0117:     Retry = 0x03,
0118:     /// 1-RTT veri paketi: kısa başlık, tam uygulama verisi.
0119:     OneRTT = 0x40,
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 02 (line 125)

```rust
0115:     Handshake = 0x02,
0116:     /// Sunucu yeniden deneme paketi: token gönderir (DDoS koruması).
0117:     Retry = 0x03,
0118:     /// 1-RTT veri paketi: kısa başlık, tam uygulama verisi.
0119:     OneRTT = 0x40,
0120: }
0121: 
0122: /// QUIC frame (çerçeve) tipleri.
0123: /// Her frame tipi farklı bir kontrol veya veri işlemi gerçekleştirir.
0124: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0125: pub enum QuicFrameType {
0126:     /// Boş dolgu: paket boyutunu arttırmak veya PMTU keşfi için.
0127:     Padding = 0x00,
0128:     /// Canlılık denetimi: ACK-eliciting (karşı taraftan ACK bekler).
0129:     Ping = 0x01,
0130:     /// Alındı onayı (ACK): hangi paketlerin alındığını bildirir.
0131:     Ack = 0x02,
0132:     /// Explicit Congestion Notification (ECN) ile ACK.
0133:     AckEcn = 0x03,
0134:     /// Akışı sıfırla: gönderme tarafı akıştan vazgeçiyor.
0135:     ResetStream = 0x04,
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 03 (line 175)

```rust
0165:     ConnectionClose = 0x1C,
0166:     /// Bağlantıyı kapat (uygulama hatası).
0167:     ConnectionCloseApp = 0x1D,
0168:     /// El sıkışmanın tamamlandığını bildir (sunucudan istemciye).
0169:     HandshakeDone = 0x1E,
0170: }
0171: 
0172: /// QUIC taşıma katmanı hata kodları (RFC 9000, Bölüm 20.1).
0173: /// ConnectionClose frame'inde error_code alanına yazılır.
0174: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0175: pub enum QuicError {
0176:     /// Hata yok; bağlantı temiz kapatıldı.
0177:     NoError = 0x00,
0178:     /// Uygulama içi beklenmedik bir hata.
0179:     InternalError = 0x01,
0180:     /// Sunucu bağlantıyı reddetti.
0181:     ConnectionRefused = 0x02,
0182:     /// Akış kontrolü sınırı aşıldı.
0183:     FlowControlError = 0x03,
0184:     /// Maksimum akış sayısı aşıldı.
0185:     StreamLimitError = 0x04,
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 04 (line 225)

```rust
0215: //
0216: // Connection ID, bir QUIC bağlantısını tanımlayan değişken uzunluklu (0-20 byte)
0217: // alandır. TCP'nin (kaynak IP, kaynak port, hedef IP, hedef port) dörtlüsünün
0218: // aksine, QUIC bağlantısı IP adresi değişse bile aynı Connection ID üzerinden
0219: // devam edebilir (bağlantı geçişi / connection migration).
0220: 
0221: /// QUIC bağlantı kimliği (değişken uzunluk, maksimum 20 byte).
0222: /// Paket başlığındaki DCID (Destination CID) ve SCID (Source CID) alanları
0223: /// bu yapı ile temsil edilir.
0224: #[derive(Clone, Debug, PartialEq, Eq)]
0225: pub struct ConnectionId {
0226:     /// Bağlantı kimliği verisi (ham byte dizisi).
0227:     pub data: Vec<u8>,
0228: }
0229: 
0230: impl ConnectionId {
0231:     /// Varolan veriden bir Connection ID oluşturur.
0232:     pub fn new(data: Vec<u8>) -> Self {
0233:         ConnectionId { data }
0234:     }
0235: 
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 05 (line 232)

```rust
0222: /// Paket başlığındaki DCID (Destination CID) ve SCID (Source CID) alanları
0223: /// bu yapı ile temsil edilir.
0224: #[derive(Clone, Debug, PartialEq, Eq)]
0225: pub struct ConnectionId {
0226:     /// Bağlantı kimliği verisi (ham byte dizisi).
0227:     pub data: Vec<u8>,
0228: }
0229: 
0230: impl ConnectionId {
0231:     /// Varolan veriden bir Connection ID oluşturur.
0232:     pub fn new(data: Vec<u8>) -> Self {
0233:         ConnectionId { data }
0234:     }
0235: 
0236:     /// Kriptografik olarak güçlü rastgele byte'lardan Connection ID üretir.
0237:     /// `len` byte, güvenli rastgele sayı üretecinden alınır.
0238:     pub fn random(len: usize) -> Self {
0239:         let mut data = vec![0u8; len];
0240:         if !crate::crypto::rdrand_bytes(&mut data) {
0241:             crate::random::fill_bytes(&mut data);
0242:         }
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 06 (line 238)

```rust
0228: }
0229: 
0230: impl ConnectionId {
0231:     /// Varolan veriden bir Connection ID oluşturur.
0232:     pub fn new(data: Vec<u8>) -> Self {
0233:         ConnectionId { data }
0234:     }
0235: 
0236:     /// Kriptografik olarak güçlü rastgele byte'lardan Connection ID üretir.
0237:     /// `len` byte, güvenli rastgele sayı üretecinden alınır.
0238:     pub fn random(len: usize) -> Self {
0239:         let mut data = vec![0u8; len];
0240:         if !crate::crypto::rdrand_bytes(&mut data) {
0241:             crate::random::fill_bytes(&mut data);
0242:         }
0243:         ConnectionId { data }
0244:     }
0245: 
0246:     /// Connection ID'nin byte uzunluğu.
0247:     pub fn len(&self) -> usize {
0248:         self.data.len()
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 07 (line 247)

```rust
0237:     /// `len` byte, güvenli rastgele sayı üretecinden alınır.
0238:     pub fn random(len: usize) -> Self {
0239:         let mut data = vec![0u8; len];
0240:         if !crate::crypto::rdrand_bytes(&mut data) {
0241:             crate::random::fill_bytes(&mut data);
0242:         }
0243:         ConnectionId { data }
0244:     }
0245: 
0246:     /// Connection ID'nin byte uzunluğu.
0247:     pub fn len(&self) -> usize {
0248:         self.data.len()
0249:     }
0250: 
0251:     /// Connection ID boş mu? (0-uzunluklu CID bazı durumlarda geçerlidir)
0252:     pub fn is_empty(&self) -> bool {
0253:         self.data.is_empty()
0254:     }
0255: 
0256:     /// Ham byte dilimini döndürür (paket serileştirme için).
0257:     pub fn as_slice(&self) -> &[u8] {
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 08 (line 252)

```rust
0242:         }
0243:         ConnectionId { data }
0244:     }
0245: 
0246:     /// Connection ID'nin byte uzunluğu.
0247:     pub fn len(&self) -> usize {
0248:         self.data.len()
0249:     }
0250: 
0251:     /// Connection ID boş mu? (0-uzunluklu CID bazı durumlarda geçerlidir)
0252:     pub fn is_empty(&self) -> bool {
0253:         self.data.is_empty()
0254:     }
0255: 
0256:     /// Ham byte dilimini döndürür (paket serileştirme için).
0257:     pub fn as_slice(&self) -> &[u8] {
0258:         &self.data
0259:     }
0260: }
0261: 
0262: // ============================================================================
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 09 (line 257)

```rust
0247:     pub fn len(&self) -> usize {
0248:         self.data.len()
0249:     }
0250: 
0251:     /// Connection ID boş mu? (0-uzunluklu CID bazı durumlarda geçerlidir)
0252:     pub fn is_empty(&self) -> bool {
0253:         self.data.is_empty()
0254:     }
0255: 
0256:     /// Ham byte dilimini döndürür (paket serileştirme için).
0257:     pub fn as_slice(&self) -> &[u8] {
0258:         &self.data
0259:     }
0260: }
0261: 
0262: // ============================================================================
0263: // QUIC AKIŞI (QUIC Stream)
0264: // ============================================================================
0265: //
0266: // QUIC akışları, tek bağlantı üzerinde birden fazla bağımsız veri kanalı sağlar.
0267: // TCP'den farklı olarak akışlar birbirini engellemez (HOL-blocking yok).
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 10 (line 285)

```rust
0275: //   2 -> İstemci açtı, tek yönlü (2, 6, 10, ...)
0276: //   3 -> Sunucu açtı, tek yönlü (3, 7, 11, ...)
0277: //
0278: // Akış Durum Makinesi:
0279: //   Idle -> Open -> HalfClosedLocal -> Closed
0280: //                -> HalfClosedRemote -> Closed
0281: //         -> ResetSent / ResetReceived
0282: 
0283: /// QUIC akış tipi.
0284: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0285: pub enum StreamType {
0286:     /// İstemci açtı, çift yönlü (HTTP/3 istekleri için).
0287:     ClientBiDi = 0,
0288:     /// Sunucu açtı, çift yönlü.
0289:     ServerBiDi = 1,
0290:     /// İstemci açtı, tek yönlü (QPACK encoder stream gibi).
0291:     ClientUniDi = 2,
0292:     /// Sunucu açtı, tek yönlü (QPACK decoder stream gibi).
0293:     ServerUniDi = 3,
0294: }
0295: 
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 11 (line 298)

```rust
0288:     /// Sunucu açtı, çift yönlü.
0289:     ServerBiDi = 1,
0290:     /// İstemci açtı, tek yönlü (QPACK encoder stream gibi).
0291:     ClientUniDi = 2,
0292:     /// Sunucu açtı, tek yönlü (QPACK decoder stream gibi).
0293:     ServerUniDi = 3,
0294: }
0295: 
0296: /// QUIC akış durumu (durum makinesi).
0297: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0298: pub enum StreamState {
0299:     /// Akış henüz açılmadı.
0300:     Idle,
0301:     /// Akış açık: her iki yönde veri akışı mümkün.
0302:     Open,
0303:     /// Yerel FIN gönderildi; yerel gönderme kapatıldı, alma devam ediyor.
0304:     HalfClosedLocal,
0305:     /// Karşı taraf FIN gönderdi; uzak gönderme kapatıldı, yerel gönderme devam ediyor.
0306:     HalfClosedRemote,
0307:     /// Her iki yön de kapatıldı.
0308:     Closed,
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 12 (line 317)

```rust
0307:     /// Her iki yön de kapatıldı.
0308:     Closed,
0309:     /// RESET_STREAM gönderildi: yerel taraf akışı iptal etti.
0310:     ResetSent,
0311:     /// RESET_STREAM alındı: uzak taraf akışı iptal etti.
0312:     ResetReceived,
0313: }
0314: 
0315: /// Tek bir QUIC akışı: gönderme ve alma tamponlarını yönetir.
0316: #[derive(Clone, Debug)]
0317: pub struct QuicStream {
0318:     /// Akış tanımlayıcısı (4 ile artan, tip bitlerini içerir).
0319:     pub id: u64,
0320:     /// Akışın tipi (istemci/sunucu, çift/tek yönlü).
0321:     pub stream_type: StreamType,
0322:     /// Akışın mevcut durumu.
0323:     pub state: StreamState,
0324:     /// Gönderme tamponu için mevcut bayt ofseti (toplam gönderilen byte).
0325:     pub send_offset: u64,
0326:     /// Alma tamponu için mevcut bayt ofseti (toplam alınan byte).
0327:     pub recv_offset: u64,
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 13 (line 345)

```rust
0335:     pub recv_buffer: Vec<u8>,
0336:     /// FIN (son byte) gönderildi mi?
0337:     pub fin_sent: bool,
0338:     /// Karşı taraftan FIN alındı mı?
0339:     pub fin_received: bool,
0340: }
0341: 
0342: impl QuicStream {
0343:     /// Yeni bir QUIC akışı oluşturur.
0344:     /// Başlangıç alma penceresi 16 MB olarak ayarlanır.
0345:     pub fn new(id: u64, stream_type: StreamType) -> Self {
0346:         QuicStream {
0347:             id,
0348:             stream_type,
0349:             state: StreamState::Idle,
0350:             send_offset: 0,
0351:             recv_offset: 0,
0352:             send_max_offset: 0,
0353:             recv_max_offset: 16 * 1024 * 1024, // 16 MB başlangıç alma penceresi
0354:             send_buffer: Vec::new(),
0355:             recv_buffer: Vec::new(),
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 14 (line 362)

```rust
0352:             send_max_offset: 0,
0353:             recv_max_offset: 16 * 1024 * 1024, // 16 MB başlangıç alma penceresi
0354:             send_buffer: Vec::new(),
0355:             recv_buffer: Vec::new(),
0356:             fin_sent: false,
0357:             fin_received: false,
0358:         }
0359:     }
0360: 
0361:     /// Akış okunabilir mi? Açık/yarı-kapalı durumda ve tampon dolu ise evet.
0362:     pub fn can_read(&self) -> bool {
0363:         matches!(self.state, StreamState::Open | StreamState::HalfClosedLocal)
0364:             && !self.recv_buffer.is_empty()
0365:     }
0366: 
0367:     /// Akışa yazılabilir mi? Açık/yarı-kapalı durumda ve gönderme penceresi dolmamışsa evet.
0368:     pub fn can_write(&self) -> bool {
0369:         matches!(
0370:             self.state,
0371:             StreamState::Open | StreamState::HalfClosedRemote
0372:         ) && self.send_offset < self.send_max_offset
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 15 (line 368)

```rust
0358:         }
0359:     }
0360: 
0361:     /// Akış okunabilir mi? Açık/yarı-kapalı durumda ve tampon dolu ise evet.
0362:     pub fn can_read(&self) -> bool {
0363:         matches!(self.state, StreamState::Open | StreamState::HalfClosedLocal)
0364:             && !self.recv_buffer.is_empty()
0365:     }
0366: 
0367:     /// Akışa yazılabilir mi? Açık/yarı-kapalı durumda ve gönderme penceresi dolmamışsa evet.
0368:     pub fn can_write(&self) -> bool {
0369:         matches!(
0370:             self.state,
0371:             StreamState::Open | StreamState::HalfClosedRemote
0372:         ) && self.send_offset < self.send_max_offset
0373:     }
0374: 
0375:     /// Akışa veri yazar. Akış kontrolü sınırına kadar yazar, fazlası kesilir.
0376:     /// Gerçekte yazılan byte sayısını döner.
0377:     pub fn write(&mut self, data: &[u8]) -> usize {
0378:         if !self.can_write() {
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 16 (line 377)

```rust
0367:     /// Akışa yazılabilir mi? Açık/yarı-kapalı durumda ve gönderme penceresi dolmamışsa evet.
0368:     pub fn can_write(&self) -> bool {
0369:         matches!(
0370:             self.state,
0371:             StreamState::Open | StreamState::HalfClosedRemote
0372:         ) && self.send_offset < self.send_max_offset
0373:     }
0374: 
0375:     /// Akışa veri yazar. Akış kontrolü sınırına kadar yazar, fazlası kesilir.
0376:     /// Gerçekte yazılan byte sayısını döner.
0377:     pub fn write(&mut self, data: &[u8]) -> usize {
0378:         if !self.can_write() {
0379:             return 0;
0380:         }
0381: 
0382:         // Kullanılabilir gönderme penceresi kadar yaz
0383:         let available = (self.send_max_offset - self.send_offset) as usize;
0384:         let to_write = data.len().min(available);
0385: 
0386:         self.send_buffer.extend_from_slice(&data[..to_write]);
0387:         self.send_offset += to_write as u64;
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 17 (line 393)

```rust
0383:         let available = (self.send_max_offset - self.send_offset) as usize;
0384:         let to_write = data.len().min(available);
0385: 
0386:         self.send_buffer.extend_from_slice(&data[..to_write]);
0387:         self.send_offset += to_write as u64;
0388: 
0389:         to_write
0390:     }
0391: 
0392:     /// Akıştan veri okur. Tamponda ne kadar varsa o kadar veya buf.len() kadar okur.
0393:     pub fn read(&mut self, buf: &mut [u8]) -> usize {
0394:         if !self.can_read() {
0395:             return 0;
0396:         }
0397: 
0398:         let to_read = buf.len().min(self.recv_buffer.len());
0399:         buf[..to_read].copy_from_slice(&self.recv_buffer[..to_read]);
0400:         // Baştan okunmuş kısmı temizle (drain: O(n) ama yeterli)
0401:         self.recv_buffer.drain(..to_read);
0402: 
0403:         to_read
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 18 (line 424)

```rust
0414: // QUIC Değişken Uzunluklu Tam Sayı (Variable-Length Integer):
0415: //   Bit 7-6'ya göre boyut belirlenir:
0416: //   00xxxxxx            -> 1 byte  (0 - 63)
0417: //   01xxxxxx xxxxxxxx   -> 2 byte  (0 - 16383)
0418: //   10xxxxxx (3 byte)   -> 4 byte  (0 - 1073741823)
0419: //   11xxxxxx (7 byte)   -> 8 byte  (0 - 4611686018427387903)
0420: 
0421: /// Tek bir QUIC frame (çerçeve). Her varyant farklı bir frame tipini temsil eder.
0422: /// Frame'ler `encode()` ile byte dizisine, `decode()` ile geri yapıya dönüştürülür.
0423: #[derive(Clone, Debug)]
0424: pub enum QuicFrame {
0425:     /// Dolgu: paket boyutunu artırmak için kullanılır.
0426:     Padding,
0427:     /// Canlılık denetimi: karşı taraftan ACK bekler.
0428:     Ping,
0429:     /// Alım onayı: hangi paket numaralarının alındığını bildirir.
0430:     Ack {
0431:         largest_ack: u64,
0432:         ack_delay: u64,
0433:         ack_range_count: u64,
0434:         first_ack_range: u64,
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 19 (line 508)

```rust
0498:     },
0499:     /// Bağlantıyı kapat (uygulama katmanı hatası).
0500:     ConnectionCloseApp { error_code: u64, reason: Vec<u8> },
0501:     /// El sıkışma tamamlandı sinyali (sunucudan istemciye).
0502:     HandshakeDone,
0503: }
0504: 
0505: impl QuicFrame {
0506:     /// Frame'i wire formatına (byte dizisi) dönüştürür.
0507:     /// QUIC değişken uzunluklu tamsayı (varint) kodlaması kullanılır.
0508:     pub fn encode(&self) -> Vec<u8> {
0509:         let mut buf = Vec::new();
0510: 
0511:         match self {
0512:             QuicFrame::Padding => {
0513:                 buf.push(QuicFrameType::Padding as u8);
0514:             }
0515:             QuicFrame::Ping => {
0516:                 buf.push(QuicFrameType::Ping as u8);
0517:             }
0518:             QuicFrame::Ack {
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 20 (line 656)

```rust
0646:         buf
0647:     }
0648: 
0649:     /// QUIC değişken uzunluklu tamsayı (varint) kodlaması.
0650:     ///
0651:     /// Değer aralığına göre 1, 2, 4 veya 8 byte kullanır:
0652:     ///   0..63        -> 1 byte (00xxxxxx)
0653:     ///   64..16383    -> 2 byte (01xxxxxx xxxxxxxx)
0654:     ///   16384..1G-1  -> 4 byte (10xxxxxx ...)
0655:     ///   1G..4.6E-1   -> 8 byte (11xxxxxx ...)
0656:     fn encode_varint(buf: &mut Vec<u8>, val: u64) {
0657:         if val < 64 {
0658:             buf.push(val as u8);
0659:         } else if val < 16384 {
0660:             buf.push(((val >> 8) as u8) | 0x40);
0661:             buf.push(val as u8);
0662:         } else if val < 1073741824 {
0663:             buf.push(((val >> 24) as u8) | 0x80);
0664:             buf.push((val >> 16) as u8);
0665:             buf.push((val >> 8) as u8);
0666:             buf.push(val as u8);
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

---

## src/net/wireguard.rs

- Satir sayisi: 808
- Derin kesit sayisi: 20

### Kesit 01 (line 88)

```rust
0078: 
0079: // ============================================================================
0080: // WIREGUARD ANAHTARI
0081: // ============================================================================
0082: 
0083: /// WireGuard Curve25519 anahtarı (32 byte)
0084: ///
0085: /// Public/private anahtar çiftleri Curve25519 eğrisi üzerinde.
0086: /// Private key clamping: bytes[0] &= 248, bytes[31] &= 127, bytes[31] |= 64
0087: #[derive(Clone, Debug)]
0088: pub struct WgKey(pub [u8; WG_KEY_SIZE]);
0089: 
0090: impl WgKey {
0091:     /// Sıfır anahtar oluştur (başlangıç/hata durumu)
0092:     pub fn new() -> Self {
0093:         Self([0u8; WG_KEY_SIZE])
0094:     }
0095: 
0096:     /// Byte dizisinden anahtar oluştur
0097:     pub fn from_bytes(bytes: [u8; WG_KEY_SIZE]) -> Self {
0098:         Self(bytes)
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 02 (line 92)

```rust
0082: 
0083: /// WireGuard Curve25519 anahtarı (32 byte)
0084: ///
0085: /// Public/private anahtar çiftleri Curve25519 eğrisi üzerinde.
0086: /// Private key clamping: bytes[0] &= 248, bytes[31] &= 127, bytes[31] |= 64
0087: #[derive(Clone, Debug)]
0088: pub struct WgKey(pub [u8; WG_KEY_SIZE]);
0089: 
0090: impl WgKey {
0091:     /// Sıfır anahtar oluştur (başlangıç/hata durumu)
0092:     pub fn new() -> Self {
0093:         Self([0u8; WG_KEY_SIZE])
0094:     }
0095: 
0096:     /// Byte dizisinden anahtar oluştur
0097:     pub fn from_bytes(bytes: [u8; WG_KEY_SIZE]) -> Self {
0098:         Self(bytes)
0099:     }
0100: 
0101:     /// Rastgele Curve25519 anahtar üret
0102:     pub fn generate() -> Self {
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 03 (line 97)

```rust
0087: #[derive(Clone, Debug)]
0088: pub struct WgKey(pub [u8; WG_KEY_SIZE]);
0089: 
0090: impl WgKey {
0091:     /// Sıfır anahtar oluştur (başlangıç/hata durumu)
0092:     pub fn new() -> Self {
0093:         Self([0u8; WG_KEY_SIZE])
0094:     }
0095: 
0096:     /// Byte dizisinden anahtar oluştur
0097:     pub fn from_bytes(bytes: [u8; WG_KEY_SIZE]) -> Self {
0098:         Self(bytes)
0099:     }
0100: 
0101:     /// Rastgele Curve25519 anahtar üret
0102:     pub fn generate() -> Self {
0103:         let mut key = [0u8; WG_KEY_SIZE];
0104:         crate::crypto::rdrand_bytes(&mut key);
0105:         // Curve25519 clamping (RFC 7748)
0106:         key[0] &= 248;
0107:         key[31] &= 127;
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 04 (line 102)

```rust
0092:     pub fn new() -> Self {
0093:         Self([0u8; WG_KEY_SIZE])
0094:     }
0095: 
0096:     /// Byte dizisinden anahtar oluştur
0097:     pub fn from_bytes(bytes: [u8; WG_KEY_SIZE]) -> Self {
0098:         Self(bytes)
0099:     }
0100: 
0101:     /// Rastgele Curve25519 anahtar üret
0102:     pub fn generate() -> Self {
0103:         let mut key = [0u8; WG_KEY_SIZE];
0104:         crate::crypto::rdrand_bytes(&mut key);
0105:         // Curve25519 clamping (RFC 7748)
0106:         key[0] &= 248;
0107:         key[31] &= 127;
0108:         key[31] |= 64;
0109:         Self(key)
0110:     }
0111: 
0112:     /// Ham byte dizisi referansı döndür
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 05 (line 113)

```rust
0103:         let mut key = [0u8; WG_KEY_SIZE];
0104:         crate::crypto::rdrand_bytes(&mut key);
0105:         // Curve25519 clamping (RFC 7748)
0106:         key[0] &= 248;
0107:         key[31] &= 127;
0108:         key[31] |= 64;
0109:         Self(key)
0110:     }
0111: 
0112:     /// Ham byte dizisi referansı döndür
0113:     pub fn as_bytes(&self) -> &[u8; WG_KEY_SIZE] {
0114:         &self.0
0115:     }
0116: }
0117: 
0118: // ============================================================================
0119: // WIREGUARD PEER (EŞ NODE)
0120: // ============================================================================
0121: 
0122: /// WireGuard ağ katılımcısı (peer/eş)
0123: ///
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 06 (line 127)

```rust
0117: 
0118: // ============================================================================
0119: // WIREGUARD PEER (EŞ NODE)
0120: // ============================================================================
0121: 
0122: /// WireGuard ağ katılımcısı (peer/eş)
0123: ///
0124: /// Her peer bir public key ile tanımlanır.
0125: /// Birden fazla peer olabilir, her biri farklı IP aralıklarına yönlendirilebilir.
0126: #[derive(Debug)]
0127: pub struct WgPeer {
0128:     /// Peer'in Curve25519 public key'i (kimlik)
0129:     pub public_key: WgKey,
0130:     /// İsteğe bağlı preshared key (ek güvenlik katmanı)
0131:     /// Kuantum bilgisayarlara karşı ek koruma sağlar
0132:     pub preshared_key: WgKey,
0133:     /// Peer'in endpoint IPv4 adresi (u32, big-endian)
0134:     pub endpoint_ip: u32,
0135:     /// Peer'in UDP port numarası
0136:     pub endpoint_port: u16,
0137:     /// Son başarılı el sıkışma zamanı (Unix timestamp)
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 07 (line 153)

```rust
0143:     /// İzin verilen IP/prefix listesi: (ip, prefix_uzunluk)
0144:     /// Örnek: [(10.0.0.2, 32), (192.168.1.0, 24)]
0145:     pub allowed_ips: Vec<(u32, u8)>, // (IP, prefix_len)
0146:     /// Kalıcı keepalive aralığı (saniye, 0 = devre dışı)
0147:     pub keepalive: AtomicU32,
0148:     /// Aktif oturum durumu (şifreleme anahtarları ve nonce)
0149:     pub session: Mutex<WgSession>,
0150: }
0151: 
0152: impl Clone for WgPeer {
0153:     fn clone(&self) -> Self {
0154:         Self {
0155:             public_key: self.public_key.clone(),
0156:             preshared_key: self.preshared_key.clone(),
0157:             endpoint_ip: self.endpoint_ip,
0158:             endpoint_port: self.endpoint_port,
0159:             last_handshake: AtomicU64::new(self.last_handshake.load(Ordering::Relaxed)),
0160:             tx_bytes: AtomicU64::new(self.tx_bytes.load(Ordering::Relaxed)),
0161:             rx_bytes: AtomicU64::new(self.rx_bytes.load(Ordering::Relaxed)),
0162:             allowed_ips: self.allowed_ips.clone(),
0163:             keepalive: AtomicU32::new(self.keepalive.load(Ordering::Relaxed)),
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 08 (line 174)

```rust
0164:             session: Mutex::new(self.session.lock().clone()),
0165:         }
0166:     }
0167: }
0168: 
0169: /// WireGuard oturum durumu
0170: ///
0171: /// Başarılı el sıkışma sonrasında her peer için bir oturum oluşturulur.
0172: /// Oturum iki yönlü simetrik anahtar içerir.
0173: #[derive(Clone, Debug)]
0174: pub struct WgSession {
0175:     /// Yerel oturum indeksi (peer'in bizim nonce'umuzu takip etmesi için)
0176:     pub local_index: u32,
0177:     /// Uzak oturum indeksi (peer'in indeksi)
0178:     pub remote_index: u32,
0179:     /// Gönderme anahtarı (ChaCha20-Poly1305 için)
0180:     pub sending_key: [u8; 32],
0181:     /// Alma anahtarı (ChaCha20-Poly1305 için)
0182:     pub receiving_key: [u8; 32],
0183:     /// Gönderme nonce sayacı (her pakette artırılır, tekrar önleme)
0184:     pub sending_nonce: u64,
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 09 (line 199)

```rust
0189:     /// Oturum kuruldu mu?
0190:     pub established: bool,
0191:     /// Initiator tarafında response bekleyen ephemeral private key
0192:     pub pending_initiator_private: [u8; 32],
0193:     /// Handshake response bekleniyor mu?
0194:     pub handshake_pending: bool,
0195: }
0196: 
0197: impl WgPeer {
0198:     /// Yeni peer oluştur (sadece public key ile)
0199:     pub fn new(public_key: WgKey) -> Self {
0200:         Self {
0201:             public_key,
0202:             preshared_key: WgKey::new(),
0203:             endpoint_ip: 0,
0204:             endpoint_port: WG_DEFAULT_PORT,
0205:             last_handshake: AtomicU64::new(0),
0206:             tx_bytes: AtomicU64::new(0),
0207:             rx_bytes: AtomicU64::new(0),
0208:             allowed_ips: Vec::new(),
0209:             keepalive: AtomicU32::new(0),
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 10 (line 229)

```rust
0219:                 pending_initiator_private: [0u8; 32],
0220:                 handshake_pending: false,
0221:             }),
0222:         }
0223:     }
0224: 
0225:     /// IP adresinin bu peer için izin verilen aralıkta olup olmadığını kontrol et
0226:     ///
0227:     /// CIDR maskeleme: mask = !0u32 >> (32 - prefix_len)
0228:     /// Örnek: prefix=24 -> mask=0x00FFFFFF -> 192.168.1.0/24 aralığı
0229:     pub fn is_allowed_ip(&self, ip: u32) -> bool {
0230:         for (allowed_ip, prefix_len) in &self.allowed_ips {
0231:             let mask = if *prefix_len == 0 {
0232:                 0
0233:             } else {
0234:                 !0u32 >> (32 - prefix_len)
0235:             };
0236:             if (ip & mask) == (*allowed_ip & mask) {
0237:                 return true;
0238:             }
0239:         }
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 11 (line 253)

```rust
0243:     /// Paketi şifreleyip transport mesajı olarak hazırla
0244:     ///
0245:     /// ## Transport Mesaj Yapısı (Tip 4)
0246:     ///
0247:     /// ```
0248:     ///  byte 0    : Mesaj tipi (0x04)
0249:     ///  byte 1-4  : Yerel oturum indeksi (little-endian)
0250:     ///  byte 5-12 : Nonce (64-bit sayaç, little-endian)
0251:     ///  byte 13+  : ChaCha20-Poly1305 şifreli veri
0252:     /// ```
0253:     pub fn encrypt_packet(&self, pkt: &[u8]) -> Result<Vec<u8>, WgError> {
0254:         let mut session = self.session.lock();
0255: 
0256:         if !session.established {
0257:             return Err(WgError::NoSession);
0258:         }
0259: 
0260:         // ChaCha20-Poly1305 encryption
0261:         let nonce = session.sending_nonce;
0262:         session.sending_nonce += 1; // Nonce sayacını artır (tekrar önleme)
0263: 
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 12 (line 297)

```rust
0287:         Ok(transport)
0288:     }
0289: 
0290:     /// Gelen transport mesajını çöz ve veriyi döndür
0291:     ///
0292:     /// ## Replay Attack Koruması
0293:     ///
0294:     /// Her paket bir nonce içerir. Alıcı, daha önce görülen
0295:     /// nonce'ları reddeder. Bu sayede eski paketlerin tekrar
0296:     /// oynatılması engellenir.
0297:     pub fn decrypt_packet(&self, pkt: &[u8]) -> Result<Vec<u8>, WgError> {
0298:         if pkt.len() < WG_TRANSPORT_HEADER_LEN + WG_TRANSPORT_TAG_LEN || pkt[0] != WG_MSG_TRANSPORT {
0299:             return Err(WgError::InvalidPacket);
0300:         }
0301: 
0302:         let mut session = self.session.lock();
0303: 
0304:         if !session.established {
0305:             return Err(WgError::NoSession);
0306:         }
0307: 
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 13 (line 361)

```rust
0351: }
0352: 
0353: // ============================================================================
0354: // WIREGUARD CİHAZI (DEVICE)
0355: // ============================================================================
0356: 
0357: /// WireGuard sanal ağ arayüzü
0358: ///
0359: /// Her WireGuard arayüzünün bir private/public key çifti ve peer listesi var.
0360: /// Linux'ta "wg0", "wg1" gibi adlarla görünür.
0361: pub struct WgDevice {
0362:     /// Arayüz adı (örn: "wg0")
0363:     pub name: String,
0364:     /// Dinleme UDP portu
0365:     pub listen_port: AtomicU32,
0366:     /// Bu cihazın Curve25519 private key'i (GİZLİ, hiç iletilmez)
0367:     pub private_key: Mutex<WgKey>,
0368:     /// Bu cihazın Curve25519 public key'i (paylaşılabilir)
0369:     pub public_key: WgKey,
0370:     /// Peer listesi: public_key -> WgPeer
0371:     pub peers: Mutex<BTreeMap<[u8; WG_KEY_SIZE], Arc<WgPeer>>>,
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 14 (line 382)

```rust
0372:     /// Firewall mark (paket etiketleme)
0373:     pub fwmark: AtomicU32,
0374:     /// Arayüz aktif mi?
0375:     pub is_up: AtomicBool,
0376:     /// İstatistikler
0377:     pub stats: Mutex<WgStats>,
0378: }
0379: 
0380: /// WireGuard istatistikleri
0381: #[derive(Clone, Debug, Default)]
0382: pub struct WgStats {
0383:     /// Toplam peer sayısı
0384:     pub peers_count: u32,
0385:     /// Toplam gönderilen byte
0386:     pub total_tx: u64,
0387:     /// Toplam alınan byte
0388:     pub total_rx: u64,
0389: }
0390: 
0391: impl WgDevice {
0392:     /// Yeni WireGuard arayüzü oluştur
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 15 (line 393)

```rust
0383:     /// Toplam peer sayısı
0384:     pub peers_count: u32,
0385:     /// Toplam gönderilen byte
0386:     pub total_tx: u64,
0387:     /// Toplam alınan byte
0388:     pub total_rx: u64,
0389: }
0390: 
0391: impl WgDevice {
0392:     /// Yeni WireGuard arayüzü oluştur
0393:     pub fn new(name: &str) -> Self {
0394:         let private_key = WgKey::generate();
0395:         // Public key = X25519(private_key, BasePoint)
0396:         let x25519_priv = crate::crypto::ed25519::X25519PrivateKey::from_bytes(private_key.0);
0397:         let public_key = WgKey::from_bytes(*x25519_priv.public_key().as_bytes());
0398: 
0399:         Self {
0400:             name: String::from(name),
0401:             listen_port: AtomicU32::new(WG_DEFAULT_PORT as u32),
0402:             private_key: Mutex::new(private_key),
0403:             public_key,
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 16 (line 412)

```rust
0402:             private_key: Mutex::new(private_key),
0403:             public_key,
0404:             peers: Mutex::new(BTreeMap::new()),
0405:             fwmark: AtomicU32::new(0),
0406:             is_up: AtomicBool::new(false),
0407:             stats: Mutex::new(WgStats::default()),
0408:         }
0409:     }
0410: 
0411:     /// Peer ekle (public key ile indekslenmiş)
0412:     pub fn add_peer(&self, peer: Arc<WgPeer>) {
0413:         self.peers.lock().insert(peer.public_key.0, peer.clone());
0414: 
0415:         let mut stats = self.stats.lock();
0416:         stats.peers_count += 1;
0417:     }
0418: 
0419:     /// Peer kaldır
0420:     pub fn remove_peer(&self, public_key: &WgKey) {
0421:         self.peers.lock().remove(&public_key.0);
0422:     }
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 17 (line 420)

```rust
0410: 
0411:     /// Peer ekle (public key ile indekslenmiş)
0412:     pub fn add_peer(&self, peer: Arc<WgPeer>) {
0413:         self.peers.lock().insert(peer.public_key.0, peer.clone());
0414: 
0415:         let mut stats = self.stats.lock();
0416:         stats.peers_count += 1;
0417:     }
0418: 
0419:     /// Peer kaldır
0420:     pub fn remove_peer(&self, public_key: &WgKey) {
0421:         self.peers.lock().remove(&public_key.0);
0422:     }
0423: 
0424:     /// Public key'e göre peer getir
0425:     pub fn get_peer(&self, public_key: &WgKey) -> Option<Arc<WgPeer>> {
0426:         self.peers.lock().get(&public_key.0).cloned()
0427:     }
0428: 
0429:     /// Allowed IP'ye göre peer bul (rota tablosu araması)
0430:     pub fn find_peer_by_ip(&self, ip: u32) -> Option<Arc<WgPeer>> {
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 18 (line 425)

```rust
0415:         let mut stats = self.stats.lock();
0416:         stats.peers_count += 1;
0417:     }
0418: 
0419:     /// Peer kaldır
0420:     pub fn remove_peer(&self, public_key: &WgKey) {
0421:         self.peers.lock().remove(&public_key.0);
0422:     }
0423: 
0424:     /// Public key'e göre peer getir
0425:     pub fn get_peer(&self, public_key: &WgKey) -> Option<Arc<WgPeer>> {
0426:         self.peers.lock().get(&public_key.0).cloned()
0427:     }
0428: 
0429:     /// Allowed IP'ye göre peer bul (rota tablosu araması)
0430:     pub fn find_peer_by_ip(&self, ip: u32) -> Option<Arc<WgPeer>> {
0431:         for peer in self.peers.lock().values() {
0432:             if peer.is_allowed_ip(ip) {
0433:                 return Some(peer.clone());
0434:             }
0435:         }
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 19 (line 430)

```rust
0420:     pub fn remove_peer(&self, public_key: &WgKey) {
0421:         self.peers.lock().remove(&public_key.0);
0422:     }
0423: 
0424:     /// Public key'e göre peer getir
0425:     pub fn get_peer(&self, public_key: &WgKey) -> Option<Arc<WgPeer>> {
0426:         self.peers.lock().get(&public_key.0).cloned()
0427:     }
0428: 
0429:     /// Allowed IP'ye göre peer bul (rota tablosu araması)
0430:     pub fn find_peer_by_ip(&self, ip: u32) -> Option<Arc<WgPeer>> {
0431:         for peer in self.peers.lock().values() {
0432:             if peer.is_allowed_ip(ip) {
0433:                 return Some(peer.clone());
0434:             }
0435:         }
0436:         None
0437:     }
0438: 
0439:     fn select_single_handshake_peer(&self) -> Result<Arc<WgPeer>, WgError> {
0440:         let peers = self.peers.lock();
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 20 (line 439)

```rust
0429:     /// Allowed IP'ye göre peer bul (rota tablosu araması)
0430:     pub fn find_peer_by_ip(&self, ip: u32) -> Option<Arc<WgPeer>> {
0431:         for peer in self.peers.lock().values() {
0432:             if peer.is_allowed_ip(ip) {
0433:                 return Some(peer.clone());
0434:             }
0435:         }
0436:         None
0437:     }
0438: 
0439:     fn select_single_handshake_peer(&self) -> Result<Arc<WgPeer>, WgError> {
0440:         let peers = self.peers.lock();
0441:         peers.values().next().cloned().ok_or(WgError::PeerNotFound)
0442:     }
0443: 
0444:     /// El sıkışma başlat
0445:     ///
0446:     /// Noise_IKpsk2 protokolüne göre:
0447:     /// 1. Geçici Curve25519 anahtar çifti üret
0448:     /// 2. ECDH(ephemeral_private, peer_public) hesapla
0449:     /// 3. Hash zincirini güncelle
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

---

## src/net/http2_huffman.rs

- Satir sayisi: 406
- Derin kesit sayisi: 11

### Kesit 01 (line 5)

```rust
0001: use alloc::collections::BTreeMap;
0002: use alloc::vec::Vec;
0003: 
0004: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0005: pub enum HuffmanDecodeError {
0006:     PaddingTooLarge,
0007:     InvalidPadding,
0008:     EosInString,
0009: }
0010: 
0011: enum HuffmanCodeSymbol {
0012:     Symbol(u8),
0013:     EndOfString,
0014: }
0015: 
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 02 (line 11)

```rust
0001: use alloc::collections::BTreeMap;
0002: use alloc::vec::Vec;
0003: 
0004: #[derive(Clone, Copy, Debug, PartialEq, Eq)]
0005: pub enum HuffmanDecodeError {
0006:     PaddingTooLarge,
0007:     InvalidPadding,
0008:     EosInString,
0009: }
0010: 
0011: enum HuffmanCodeSymbol {
0012:     Symbol(u8),
0013:     EndOfString,
0014: }
0015: 
0016: impl HuffmanCodeSymbol {
0017:     fn new(symbol: usize) -> Self {
0018:         if symbol == 256 {
0019:             Self::EndOfString
0020:         } else {
0021:             Self::Symbol(symbol as u8)
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 03 (line 17)

```rust
0007:     InvalidPadding,
0008:     EosInString,
0009: }
0010: 
0011: enum HuffmanCodeSymbol {
0012:     Symbol(u8),
0013:     EndOfString,
0014: }
0015: 
0016: impl HuffmanCodeSymbol {
0017:     fn new(symbol: usize) -> Self {
0018:         if symbol == 256 {
0019:             Self::EndOfString
0020:         } else {
0021:             Self::Symbol(symbol as u8)
0022:         }
0023:     }
0024: }
0025: 
0026: struct HuffmanDecoder {
0027:     table: BTreeMap<u8, BTreeMap<u32, HuffmanCodeSymbol>>,
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 04 (line 26)

```rust
0016: impl HuffmanCodeSymbol {
0017:     fn new(symbol: usize) -> Self {
0018:         if symbol == 256 {
0019:             Self::EndOfString
0020:         } else {
0021:             Self::Symbol(symbol as u8)
0022:         }
0023:     }
0024: }
0025: 
0026: struct HuffmanDecoder {
0027:     table: BTreeMap<u8, BTreeMap<u32, HuffmanCodeSymbol>>,
0028:     eos_codepoint: (u32, u8),
0029: }
0030: 
0031: impl HuffmanDecoder {
0032:     fn from_table(table: &[(u32, u8)]) -> Self {
0033:         let mut decoder_table = BTreeMap::new();
0034:         let mut eos_codepoint = None;
0035: 
0036:         for (symbol, &(code, code_len)) in table.iter().enumerate() {
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 05 (line 32)

```rust
0022:         }
0023:     }
0024: }
0025: 
0026: struct HuffmanDecoder {
0027:     table: BTreeMap<u8, BTreeMap<u32, HuffmanCodeSymbol>>,
0028:     eos_codepoint: (u32, u8),
0029: }
0030: 
0031: impl HuffmanDecoder {
0032:     fn from_table(table: &[(u32, u8)]) -> Self {
0033:         let mut decoder_table = BTreeMap::new();
0034:         let mut eos_codepoint = None;
0035: 
0036:         for (symbol, &(code, code_len)) in table.iter().enumerate() {
0037:             decoder_table
0038:                 .entry(code_len)
0039:                 .or_insert_with(BTreeMap::new)
0040:                 .insert(code, HuffmanCodeSymbol::new(symbol));
0041:             if symbol == 256 {
0042:                 eos_codepoint = Some((code, code_len));
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 06 (line 52)

```rust
0042:                 eos_codepoint = Some((code, code_len));
0043:             }
0044:         }
0045: 
0046:         Self {
0047:             table: decoder_table,
0048:             eos_codepoint: eos_codepoint.expect("HPACK EOS codepoint"),
0049:         }
0050:     }
0051: 
0052:     fn new() -> Self {
0053:         Self::from_table(HPACK_HUFFMAN_CODE_TABLE)
0054:     }
0055: 
0056:     fn decode(&mut self, buf: &[u8]) -> Result<Vec<u8>, HuffmanDecodeError> {
0057:         let mut current: u32 = 0;
0058:         let mut current_len: u8 = 0;
0059:         let mut result = Vec::new();
0060: 
0061:         for bit in BitIterator::new(buf.iter()) {
0062:             current_len += 1;
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 07 (line 56)

```rust
0046:         Self {
0047:             table: decoder_table,
0048:             eos_codepoint: eos_codepoint.expect("HPACK EOS codepoint"),
0049:         }
0050:     }
0051: 
0052:     fn new() -> Self {
0053:         Self::from_table(HPACK_HUFFMAN_CODE_TABLE)
0054:     }
0055: 
0056:     fn decode(&mut self, buf: &[u8]) -> Result<Vec<u8>, HuffmanDecodeError> {
0057:         let mut current: u32 = 0;
0058:         let mut current_len: u8 = 0;
0059:         let mut result = Vec::new();
0060: 
0061:         for bit in BitIterator::new(buf.iter()) {
0062:             current_len += 1;
0063:             current <<= 1;
0064:             if bit {
0065:                 current |= 1;
0066:             }
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 08 (line 106)

```rust
0096:         };
0097: 
0098:         if (right_align_eos & mask) != right_align_current {
0099:             return Err(HuffmanDecodeError::InvalidPadding);
0100:         }
0101: 
0102:         Ok(result)
0103:     }
0104: }
0105: 
0106: struct BitIterator<'a, I: Iterator<Item = &'a u8>> {
0107:     buffer_iterator: I,
0108:     current_byte: Option<&'a u8>,
0109:     pos: u8,
0110: }
0111: 
0112: impl<'a, I: Iterator<Item = &'a u8>> BitIterator<'a, I> {
0113:     fn new(iterator: I) -> Self {
0114:         Self {
0115:             buffer_iterator: iterator,
0116:             current_byte: None,
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 09 (line 113)

```rust
0103:     }
0104: }
0105: 
0106: struct BitIterator<'a, I: Iterator<Item = &'a u8>> {
0107:     buffer_iterator: I,
0108:     current_byte: Option<&'a u8>,
0109:     pos: u8,
0110: }
0111: 
0112: impl<'a, I: Iterator<Item = &'a u8>> BitIterator<'a, I> {
0113:     fn new(iterator: I) -> Self {
0114:         Self {
0115:             buffer_iterator: iterator,
0116:             current_byte: None,
0117:             pos: 7,
0118:         }
0119:     }
0120: }
0121: 
0122: impl<'a, I: Iterator<Item = &'a u8>> Iterator for BitIterator<'a, I> {
0123:     type Item = bool;
```

Matematiksel cerceve:

\[L_{tail}=\operatorname{p99}(L)\]

\[R=\frac{ok}{ok+err}\]

\[G=\Delta perf-\lambda\Delta risk\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 10 (line 125)

```rust
0115:             buffer_iterator: iterator,
0116:             current_byte: None,
0117:             pos: 7,
0118:         }
0119:     }
0120: }
0121: 
0122: impl<'a, I: Iterator<Item = &'a u8>> Iterator for BitIterator<'a, I> {
0123:     type Item = bool;
0124: 
0125:     fn next(&mut self) -> Option<Self::Item> {
0126:         if self.current_byte.is_none() {
0127:             self.current_byte = self.buffer_iterator.next();
0128:             self.pos = 7;
0129:         }
0130: 
0131:         self.current_byte?;
0132: 
0133:         let byte = *self.current_byte.unwrap();
0134:         let is_set = (byte & (1 << self.pos)) == (1 << self.pos);
0135:         if self.pos == 0 {
```

Matematiksel cerceve:

\[U=\sum_i\frac{C_i}{T_i}\]

\[J=\max_i q_i-\min_i q_i\]

\[S=\frac{throughput}{cpu}\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

### Kesit 11 (line 144)

```rust
0134:         let is_set = (byte & (1 << self.pos)) == (1 << self.pos);
0135:         if self.pos == 0 {
0136:             self.current_byte = None;
0137:         } else {
0138:             self.pos -= 1;
0139:         }
0140:         Some(is_set)
0141:     }
0142: }
0143: 
0144: pub fn decode_huffman(buf: &[u8]) -> Result<Vec<u8>, HuffmanDecodeError> {
0145:     HuffmanDecoder::new().decode(buf)
0146: }
0147: 
0148: static HPACK_HUFFMAN_CODE_TABLE: &[(u32, u8)] = &[
0149:     (0x1ff8, 13),
0150:     (0x7fffd8, 23),
0151:     (0xfffffe2, 28),
0152:     (0xfffffe3, 28),
0153:     (0xfffffe4, 28),
0154:     (0xfffffe5, 28),
```

Matematiksel cerceve:

\[P_{fail}=1-\prod_i(1-p_i)\]

\[M=\frac{used}{cap}\]

\[C_{tot}=\sum_i C_i\]

Kod-matematik baglami:

Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini nicel dille ifade eder.

---
