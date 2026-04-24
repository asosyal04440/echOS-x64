# Cilt 1 Core Sembol Dokumu

Bu bolum, Cilt 1 kapsamindaki cekirdek dosyalarin sembol envanterini tek bir teknik dokumde toplar. Hedef, API yuzeyi, internal yardimci fonksiyonlar ve veri-tip kontratlarini sayisal bir cizelgeyle okumayi kolaylastirmaktir.

Okuma protokolu:

1. Dosya bazli sembol yogunlugunu incele.
2. Public/internal dagilimini ownership modeliyle eslestir.
3. Buyuk kontrat yuzeyleri icin degisim riski notunu cikar.

---

## src/main.rs

- Satir sayisi: 2079
- Toplam sembol: 39
- Public sembol: 0
- Fonksiyon: 27, Struct: 2, Enum: 0, Const: 10

### Sembol envanteri

| Kind | Name | Line | Visibility |
|---|---|---:|---|
| const | `COM1` | 76 | internal |
| const | `BOOT_MAGIC_UEFI` | 77 | internal |
| const | `BOOT_MAGIC_MB2` | 78 | internal |
| const | `CMDLINE_MAX_LEN` | 80 | internal |
| const | `SECURE_BOOT_ENROLL_MAGIC` | 82 | internal |
| const | `SECURE_BOOT_ENROLL_PENDING_RESET` | 84 | internal |
| const | `SECURE_BOOT_ENROLL_FAILED` | 86 | internal |
| const | `LIMINE_REVISION` | 88 | internal |
| struct | `SecureBootEnrollState` | 93 | internal |
| fn | `serial_write_byte` | 158 | internal |
| struct | `SerialPort` | 172 | internal |
| fn | `write_str` | 175 | internal |
| fn | `serial_write_str` | 183 | internal |
| fn | `init_platform_iommu` | 188 | internal |
| fn | `parse_swap_cmdline` | 221 | internal |
| fn | `panic` | 260 | internal |
| const | `PREFERRED_GOP_WIDTH` | 416 | internal |
| const | `PREFERRED_GOP_HEIGHT` | 418 | internal |
| fn | `gop_mode_rank` | 421 | internal |
| fn | `configure_preferred_gop_mode` | 432 | internal |
| fn | `limine_available` | 559 | internal |
| fn | `detect_secure_boot` | 1393 | internal |
| fn | `read_global_u8_variable` | 1403 | internal |
| fn | `appliance_variable_vendor` | 1414 | internal |
| fn | `read_boot_control_variable_seed` | 1426 | internal |
| fn | `curated_app_bundle_path` | 1441 | internal |
| fn | `read_efi_boot_file` | 1480 | internal |
| fn | `efi_boot_file_size` | 1514 | internal |
| fn | `read_boot_control_seed` | 1538 | internal |
| fn | `sync_boot_control_seed` | 1551 | internal |
| fn | `read_secure_boot_enroll_state` | 1601 | internal |
| fn | `write_secure_boot_enroll_state` | 1619 | internal |
| fn | `write_global_variable_payload` | 1648 | internal |
| fn | `auto_enroll_secure_boot_payloads` | 1668 | internal |
| fn | `inspect_loaded_image` | 1757 | internal |
| fn | `measure_loaded_image_tpm` | 1795 | internal |
| fn | `measure_cmdline_tpm` | 1885 | internal |
| fn | `report_tpm_event_log` | 1956 | internal |
| fn | `read_cmdline` | 2041 | internal |

### Muhendislik notu

Bu dosyada API yuzeyi ve internal yardimci fonksiyonlarin dagilimi, kodun sahiplik ve publication sinirlarini yorumlamak icin ilk sinyal katmanidir. Sembol yogunlugu tek basina kalite olcutu degildir; ancak degisen release'lerde kontrat yuzeyinin hangi dosyada buyudugunu sayisal olarak izlemeyi mumkun kilar.

---

## src/task/scheduler.rs

- Satir sayisi: 1948
- Toplam sembol: 99
- Public sembol: 68
- Fonksiyon: 82, Struct: 5, Enum: 1, Const: 11

### Sembol envanteri

| Kind | Name | Line | Visibility |
|---|---|---:|---|
| const | `MAX_CPUS` | 66 | internal |
| const | `MSR_GS_BASE` | 67 | internal |
| struct | `SmpScheduler` | 80 | pub |
| fn | `new` | 85 | pub |
| fn | `allocate_task_id` | 91 | pub |
| fn | `spawn` | 95 | pub |
| fn | `spawn_boxed` | 106 | pub |
| fn | `task_can_run_on_cpu` | 116 | internal |
| fn | `queued_task_count_usize` | 120 | internal |
| fn | `queued_task_count` | 130 | pub |
| fn | `publish_worker_load` | 134 | internal |
| fn | `choose_spawn_cpu` | 138 | internal |
| fn | `enqueue_boxed_task` | 165 | internal |
| const | `NICE_0_LOAD` | 208 | internal |
| const | `SCHED_LATENCY_TICKS` | 209 | internal |
| const | `MIN_GRANULARITY_TICKS` | 210 | internal |
| const | `LOAD_BALANCE_INTERVAL` | 211 | internal |
| const | `VRUNTIME_NORMALIZE_INTERVAL` | 212 | internal |
| enum | `SchedulerPressureClass` | 215 | internal |
| struct | `SchedulerPressureSnapshot` | 222 | internal |
| fn | `init` | 233 | pub |
| fn | `get_cpu_load` | 282 | pub |
| fn | `update_cpu_count` | 296 | pub |
| fn | `enable_secondary_scheduling` | 318 | pub |
| fn | `secondary_scheduling_active` | 322 | pub |
| fn | `current_kernel_stack_top` | 326 | pub |
| fn | `classify_current_kernel_stack_fault` | 343 | pub |
| fn | `record_current_stack_pointer` | 357 | pub |
| fn | `current_kernel_stack_usage` | 374 | pub |
| fn | `current_user_target` | 392 | pub |
| fn | `current_win32_thread_state` | 407 | pub |
| fn | `current_task_id` | 427 | pub |
| fn | `current_execution_mode` | 438 | pub |
| fn | `current_user_page_table` | 448 | pub |
| fn | `current_address_space` | 458 | pub |
| fn | `task_exists` | 468 | pub |
| fn | `fork_current_user_task` | 486 | pub |
| fn | `idle_loop` | 522 | pub |
| fn | `spawn` | 542 | pub |
| fn | `spawn_with_priority` | 547 | pub |
| fn | `spawn_with_priority_in_address_space` | 569 | pub |
| fn | `get_ticks` | 594 | pub |
| fn | `is_ready` | 598 | pub |
| fn | `tick` | 606 | pub |
| const | `LOAD_UPDATE_INTERVAL` | 630 | internal |
| const | `LOAD_UPDATE_INTERVAL` | 632 | internal |
| const | `BALANCE_INTERVAL` | 647 | internal |
| const | `BALANCE_INTERVAL` | 649 | internal |
| fn | `should_preempt` | 655 | internal |
| fn | `scheduler_pressure_snapshot` | 669 | internal |
| fn | `calc_time_slice` | 685 | internal |
| fn | `update_task_vruntime` | 712 | internal |
| fn | `wake_sleeping_tasks` | 720 | internal |
| fn | `sleep` | 735 | pub |
| fn | `exit` | 754 | pub |
| fn | `wait_for_terminated` | 782 | pub |
| fn | `get_current_ptrace_flags` | 803 | pub |
| fn | `set_ptrace_flag` | 817 | pub |
| fn | `get_current_seccomp_mode` | 830 | pub |
| fn | `set_current_seccomp_mode` | 844 | pub |
| fn | `exec_current_user_image` | 854 | pub |
| fn | `spawn_user_image_task` | 890 | pub |
| fn | `spawn_user_image_task_with_address_space` | 898 | pub |
| fn | `get_current_cpu_id` | 967 | internal |
| fn | `has_schedulable_work` | 971 | internal |
| fn | `choose_victim_cpu` | 982 | internal |
| fn | `restore_worker_tasks` | 1044 | internal |
| fn | `take_task_from_worker_by_id` | 1052 | internal |
| fn | `steal_task_from_victim_by_id` | 1073 | internal |
| fn | `take_committed_policy_task` | 1100 | internal |
| fn | `schedule` | 1127 | pub |
| fn | `switch_context` | 1523 | internal |
| struct | `TaskInfo` | 1550 | pub |
| fn | `list_tasks` | 1559 | pub |
| fn | `kill_task` | 1594 | pub |
| fn | `stop_task` | 1643 | pub |
| fn | `continue_task` | 1648 | pub |
| fn | `background_current` | 1653 | pub |
| fn | `get_foreground_task` | 1668 | pub |
| fn | `foreground_task` | 1687 | pub |
| fn | `get_task_state` | 1709 | pub |
| fn | `get_cpu_count` | 1723 | pub |
| fn | `steal_from_cpu` | 1729 | pub |
| fn | `push_to_cpu` | 1741 | pub |
| struct | `SchedulerStats` | 1754 | pub |
| fn | `get_stats` | 1762 | pub |
| fn | `process_deferred_timers` | 1794 | pub |
| struct | `WaitQueue` | 1816 | pub |
| fn | `new` | 1821 | pub |
| fn | `sleep` | 1830 | pub |
| fn | `wake_one` | 1849 | pub |
| fn | `wake_all` | 1862 | pub |
| fn | `waiter_count` | 1873 | pub |
| fn | `has_waiters` | 1878 | pub |
| fn | `take_current_blocked_task` | 1885 | pub |
| fn | `wake_blocked_task` | 1898 | pub |
| fn | `spawn_task` | 1905 | pub |
| fn | `block_current_task` | 1913 | pub |
| fn | `unblock_task` | 1930 | pub |

### Muhendislik notu

Bu dosyada API yuzeyi ve internal yardimci fonksiyonlarin dagilimi, kodun sahiplik ve publication sinirlarini yorumlamak icin ilk sinyal katmanidir. Sembol yogunlugu tek basina kalite olcutu degildir; ancak degisen release'lerde kontrat yuzeyinin hangi dosyada buyudugunu sayisal olarak izlemeyi mumkun kilar.

---

## src/task/rt_scheduler.rs

- Satir sayisi: 628
- Toplam sembol: 55
- Public sembol: 49
- Fonksiyon: 46, Struct: 3, Enum: 1, Const: 5

### Sembol envanteri

| Kind | Name | Line | Visibility |
|---|---|---:|---|
| const | `RT_PRIO_MIN` | 59 | pub |
| const | `RT_PRIO_MAX` | 62 | pub |
| const | `RR_DEFAULT_TIMESLICE` | 66 | pub |
| const | `RR_MAX_TIMESLICE` | 69 | pub |
| const | `RR_MIN_TIMESLICE` | 72 | pub |
| enum | `SchedPolicy` | 81 | pub |
| fn | `default` | 97 | internal |
| struct | `RtSchedParam` | 104 | pub |
| fn | `default` | 116 | internal |
| struct | `RtTaskInfo` | 132 | pub |
| fn | `new` | 147 | pub |
| fn | `with_rt` | 159 | pub |
| fn | `calculate_timeslice` | 180 | internal |
| fn | `reset_timeslice` | 188 | pub |
| fn | `tick` | 194 | pub |
| struct | `RtRunQueue` | 214 | pub |
| fn | `new` | 231 | pub |
| fn | `enqueue` | 244 | pub |
| fn | `dequeue` | 269 | pub |
| fn | `pick_next` | 295 | pub |
| fn | `find_highest_prio` | 323 | internal |
| fn | `update_highest_prio` | 334 | internal |
| fn | `rt_task_count` | 340 | pub |
| fn | `has_rt_tasks` | 345 | pub |
| fn | `set_sched_param` | 350 | pub |
| fn | `get_sched_param` | 379 | pub |
| fn | `tick` | 395 | pub |
| fn | `reenqueue_rr` | 406 | pub |
| fn | `set_rt_bandwidth` | 417 | pub |
| fn | `set_rt_throttling` | 423 | pub |
| fn | `default` | 429 | internal |
| fn | `init` | 448 | pub |
| fn | `has_rt_tasks` | 453 | pub |
| fn | `rt_task_count` | 458 | pub |
| fn | `enqueue_rt_task` | 463 | pub |
| fn | `dequeue_rt_task` | 468 | pub |
| fn | `pick_next_rt_task` | 474 | pub |
| fn | `set_sched_param` | 479 | pub |
| fn | `get_sched_param` | 484 | pub |
| fn | `rt_tick` | 490 | pub |
| fn | `reenqueue_rr_task` | 495 | pub |
| fn | `set_rt_bandwidth` | 500 | pub |
| fn | `set_rt_throttling` | 505 | pub |
| fn | `is_rt_task` | 510 | pub |
| fn | `get_task_priority` | 520 | pub |
| fn | `get_task_policy` | 530 | pub |
| fn | `yield_rt_task` | 542 | pub |
| fn | `sys_sched_setscheduler` | 559 | pub |
| fn | `sys_sched_getscheduler` | 581 | pub |
| fn | `sys_sched_setparam` | 586 | pub |
| fn | `sys_sched_getparam` | 593 | pub |
| fn | `sys_sched_yield` | 598 | pub |
| fn | `sys_sched_get_priority_max` | 604 | pub |
| fn | `sys_sched_get_priority_min` | 612 | pub |
| fn | `sys_sched_rr_get_interval` | 621 | pub |

### Muhendislik notu

Bu dosyada API yuzeyi ve internal yardimci fonksiyonlarin dagilimi, kodun sahiplik ve publication sinirlarini yorumlamak icin ilk sinyal katmanidir. Sembol yogunlugu tek basina kalite olcutu degildir; ancak degisen release'lerde kontrat yuzeyinin hangi dosyada buyudugunu sayisal olarak izlemeyi mumkun kilar.

---

## src/task/cfs.rs

- Satir sayisi: 481
- Toplam sembol: 36
- Public sembol: 36
- Fonksiyon: 26, Struct: 4, Enum: 0, Const: 6

### Sembol envanteri

| Kind | Name | Line | Visibility |
|---|---|---:|---|
| const | `CFS_DEFAULT_SLICE` | 49 | pub |
| const | `CFS_MIN_GRANULARITY` | 51 | pub |
| const | `CFS_WAKEUP_GRANULARITY` | 53 | pub |
| const | `CFS_NICE_0_WEIGHT` | 55 | pub |
| const | `CFS_LOAD_AVG_PERIOD` | 57 | pub |
| const | `CFS_PELT_HALF_LIFE` | 59 | pub |
| fn | `nice_to_weight` | 64 | pub |
| fn | `weight_to_vruntime` | 82 | pub |
| struct | `CfsTask` | 94 | pub |
| struct | `CfsStats` | 122 | pub |
| fn | `new` | 134 | pub |
| fn | `set_nice` | 152 | pub |
| fn | `update_vruntime` | 159 | pub |
| fn | `calc_slice` | 168 | pub |
| fn | `is_eligible` | 181 | pub |
| struct | `CfsRq` | 205 | pub |
| fn | `new` | 225 | pub |
| fn | `enqueue` | 241 | pub |
| fn | `dequeue` | 258 | pub |
| fn | `pick_next` | 269 | pub |
| fn | `put_prev` | 287 | pub |
| fn | `update_clock` | 298 | pub |
| fn | `update_load_avg` | 304 | pub |
| fn | `check_preempt_wakeup` | 315 | pub |
| struct | `CfsScheduler` | 334 | pub |
| fn | `new` | 348 | pub |
| fn | `schedule` | 364 | pub |
| fn | `tick` | 375 | pub |
| fn | `enqueue` | 401 | pub |
| fn | `dequeue` | 409 | pub |
| fn | `load_balance` | 418 | pub |
| fn | `set_nice` | 447 | pub |
| fn | `sys_sched_setparam` | 460 | pub |
| fn | `sys_sched_getparam` | 466 | pub |
| fn | `sys_sched_yield` | 470 | pub |
| fn | `init` | 479 | pub |

### Muhendislik notu

Bu dosyada API yuzeyi ve internal yardimci fonksiyonlarin dagilimi, kodun sahiplik ve publication sinirlarini yorumlamak icin ilk sinyal katmanidir. Sembol yogunlugu tek basina kalite olcutu degildir; ancak degisen release'lerde kontrat yuzeyinin hangi dosyada buyudugunu sayisal olarak izlemeyi mumkun kilar.

---

## src/task/eevdf.rs

- Satir sayisi: 183
- Toplam sembol: 15
- Public sembol: 14
- Fonksiyon: 11, Struct: 4, Enum: 0, Const: 0

### Sembol envanteri

| Kind | Name | Line | Visibility |
|---|---|---:|---|
| struct | `EevdfTask` | 17 | pub |
| fn | `new` | 29 | pub |
| fn | `update_runtime` | 44 | pub |
| struct | `DeadlineKey` | 58 | internal |
| struct | `EevdfStats` | 64 | pub |
| struct | `EevdfRunQueue` | 70 | pub |
| fn | `new` | 77 | pub |
| fn | `vtime` | 85 | pub |
| fn | `enqueue` | 89 | pub |
| fn | `dequeue` | 101 | pub |
| fn | `account_runtime` | 112 | pub |
| fn | `pick_next` | 135 | pub |
| fn | `should_preempt` | 145 | pub |
| fn | `stats` | 162 | pub |
| fn | `ordered_task_ids` | 176 | pub |

### Muhendislik notu

Bu dosyada API yuzeyi ve internal yardimci fonksiyonlarin dagilimi, kodun sahiplik ve publication sinirlarini yorumlamak icin ilk sinyal katmanidir. Sembol yogunlugu tek basina kalite olcutu degildir; ancak degisen release'lerde kontrat yuzeyinin hangi dosyada buyudugunu sayisal olarak izlemeyi mumkun kilar.

---

## src/task/deadline.rs

- Satir sayisi: 464
- Toplam sembol: 36
- Public sembol: 35
- Fonksiyon: 23, Struct: 5, Enum: 1, Const: 7

### Sembol envanteri

| Kind | Name | Line | Visibility |
|---|---|---:|---|
| const | `DL_DEFAULT_RUNTIME` | 59 | pub |
| const | `DL_DEFAULT_PERIOD` | 61 | pub |
| const | `DL_DEFAULT_DEADLINE` | 63 | pub |
| const | `SCHED_DEADLINE` | 66 | pub |
| const | `SCHED_FLAG_DL_OVERRUN` | 69 | pub |
| const | `SCHED_FLAG_DL_RECLAIM` | 70 | pub |
| const | `SCHED_FLAG_DL_SPECIAL` | 71 | pub |
| struct | `DeadlineTask` | 83 | pub |
| struct | `DlStats` | 109 | pub |
| fn | `new` | 117 | pub |
| fn | `deadline_passed` | 137 | pub |
| fn | `runtime_exhausted` | 143 | pub |
| fn | `consume_runtime` | 149 | pub |
| fn | `replenish` | 161 | pub |
| fn | `laxity` | 184 | pub |
| fn | `compare_deadline` | 193 | pub |
| struct | `DeadlineRq` | 213 | pub |
| fn | `new` | 225 | pub |
| fn | `enqueue` | 236 | pub |
| fn | `dequeue` | 254 | pub |
| fn | `pick_next` | 264 | pub |
| fn | `compute_bandwidth` | 280 | internal |
| fn | `check_replenishments` | 293 | pub |
| fn | `check_deadline_misses` | 304 | pub |
| struct | `DeadlineScheduler` | 323 | pub |
| fn | `new` | 335 | pub |
| fn | `schedule` | 351 | pub |
| fn | `add_task` | 362 | pub |
| fn | `remove_task` | 372 | pub |
| fn | `tick` | 381 | pub |
| fn | `set_bandwidth_cap` | 400 | pub |
| enum | `DlError` | 417 | pub |
| fn | `sys_sched_setattr` | 428 | pub |
| fn | `sys_sched_getattr` | 438 | pub |
| struct | `SchedAttr` | 448 | pub |
| fn | `init` | 462 | pub |

### Muhendislik notu

Bu dosyada API yuzeyi ve internal yardimci fonksiyonlarin dagilimi, kodun sahiplik ve publication sinirlarini yorumlamak icin ilk sinyal katmanidir. Sembol yogunlugu tek basina kalite olcutu degildir; ancak degisen release'lerde kontrat yuzeyinin hangi dosyada buyudugunu sayisal olarak izlemeyi mumkun kilar.

---

## src/task/deque.rs

- Satir sayisi: 201
- Toplam sembol: 9
- Public sembol: 7
- Fonksiyon: 5, Struct: 3, Enum: 0, Const: 1

### Sembol envanteri

| Kind | Name | Line | Visibility |
|---|---|---:|---|
| const | `DEQUE_SIZE` | 42 | internal |
| struct | `Worker` | 46 | pub |
| struct | `Stealer` | 52 | pub |
| struct | `Inner` | 57 | internal |
| fn | `new` | 70 | pub |
| fn | `push` | 93 | pub |
| fn | `pop` | 118 | pub |
| fn | `len` | 163 | pub |
| fn | `steal` | 179 | pub |

### Muhendislik notu

Bu dosyada API yuzeyi ve internal yardimci fonksiyonlarin dagilimi, kodun sahiplik ve publication sinirlarini yorumlamak icin ilk sinyal katmanidir. Sembol yogunlugu tek basina kalite olcutu degildir; ancak degisen release'lerde kontrat yuzeyinin hangi dosyada buyudugunu sayisal olarak izlemeyi mumkun kilar.

---

## src/task/timer.rs

- Satir sayisi: 171
- Toplam sembol: 9
- Public sembol: 4
- Fonksiyon: 4, Struct: 1, Enum: 0, Const: 3

### Sembol envanteri

| Kind | Name | Line | Visibility |
|---|---|---:|---|
| type | `TimerBucket` | 41 | internal |
| const | `WHEEL_SIZE` | 43 | internal |
| const | `WHEEL_MASK` | 44 | internal |
| const | `WHEEL_BITS` | 45 | internal |
| struct | `TimingWheel` | 53 | pub |
| fn | `new` | 62 | pub |
| fn | `schedule` | 88 | pub |
| fn | `tick` | 122 | pub |
| fn | `cascade` | 152 | internal |

### Muhendislik notu

Bu dosyada API yuzeyi ve internal yardimci fonksiyonlarin dagilimi, kodun sahiplik ve publication sinirlarini yorumlamak icin ilk sinyal katmanidir. Sembol yogunlugu tek basina kalite olcutu degildir; ancak degisen release'lerde kontrat yuzeyinin hangi dosyada buyudugunu sayisal olarak izlemeyi mumkun kilar.

---

## src/memory/fibonacci_pmm.rs

- Satir sayisi: 451
- Toplam sembol: 24
- Public sembol: 13
- Fonksiyon: 19, Struct: 2, Enum: 1, Const: 2

### Sembol envanteri

| Kind | Name | Line | Visibility |
|---|---|---:|---|
| enum | `MemoryZone` | 71 | pub |
| const | `ZONE_DMA_LIMIT` | 81 | internal |
| const | `ZONE_DMA32_LIMIT` | 82 | internal |
| fn | `from_addr` | 86 | internal |
| fn | `fallback` | 97 | internal |
| struct | `RegionAllocator` | 110 | internal |
| struct | `FibonacciPmm` | 122 | pub |
| fn | `empty` | 138 | pub |
| fn | `zone_idx` | 149 | internal |
| fn | `allocate_from_zone` | 223 | pub |
| fn | `allocate_contiguous_from_zone` | 240 | pub |
| fn | `try_allocate_zone` | 264 | internal |
| fn | `try_allocate_contiguous_zone` | 279 | internal |
| fn | `allocate_frame` | 303 | pub |
| fn | `allocate_contiguous` | 308 | pub |
| fn | `deallocate_contiguous` | 313 | pub |
| fn | `utilization` | 337 | pub |
| fn | `total_frames` | 344 | pub |
| fn | `free_frames` | 348 | pub |
| fn | `zone_stats` | 353 | pub |
| fn | `fragmentation` | 360 | pub |
| fn | `allocate_frame` | 380 | internal |
| fn | `test_fibonacci_pmm_allocation` | 395 | internal |
| fn | `test_zone_allocation` | 417 | internal |

### Muhendislik notu

Bu dosyada API yuzeyi ve internal yardimci fonksiyonlarin dagilimi, kodun sahiplik ve publication sinirlarini yorumlamak icin ilk sinyal katmanidir. Sembol yogunlugu tek basina kalite olcutu degildir; ancak degisen release'lerde kontrat yuzeyinin hangi dosyada buyudugunu sayisal olarak izlemeyi mumkun kilar.

---

## src/memory/fibonacci_buddy.rs

- Satir sayisi: 307
- Toplam sembol: 16
- Public sembol: 6
- Fonksiyon: 13, Struct: 1, Enum: 0, Const: 2

### Sembol envanteri

| Kind | Name | Line | Visibility |
|---|---|---:|---|
| const | `PAGE_SIZE` | 56 | internal |
| const | `FIBONACCI_SERIES` | 61 | internal |
| struct | `FibonacciBuddyAllocator` | 68 | pub |
| fn | `new` | 83 | pub |
| fn | `find_fib_index` | 109 | internal |
| fn | `find_fib_index_floor` | 118 | internal |
| fn | `allocate` | 132 | pub |
| fn | `deallocate` | 159 | pub |
| fn | `find_buddy` | 172 | internal |
| fn | `split_block` | 183 | internal |
| fn | `try_coalesce` | 206 | internal |
| fn | `find_block_in_freelist` | 229 | internal |
| fn | `utilization` | 240 | pub |
| fn | `fragmentation` | 250 | pub |
| fn | `test_fibonacci_allocation` | 276 | internal |
| fn | `test_buddy_coalescing` | 294 | internal |

### Muhendislik notu

Bu dosyada API yuzeyi ve internal yardimci fonksiyonlarin dagilimi, kodun sahiplik ve publication sinirlarini yorumlamak icin ilk sinyal katmanidir. Sembol yogunlugu tek basina kalite olcutu degildir; ancak degisen release'lerde kontrat yuzeyinin hangi dosyada buyudugunu sayisal olarak izlemeyi mumkun kilar.

---

## src/allocator/tlsf.rs

- Satir sayisi: 555
- Toplam sembol: 22
- Public sembol: 13
- Fonksiyon: 14, Struct: 5, Enum: 0, Const: 3

### Sembol envanteri

| Kind | Name | Line | Visibility |
|---|---|---:|---|
| const | `EARLY_HEAP_SIZE` | 57 | internal |
| const | `HEAP_CANARY_MAGIC` | 64 | internal |
| const | `MAX_TRACKED_ALLOCATIONS` | 70 | internal |
| struct | `AllocationEntry` | 114 | internal |
| fn | `new` | 122 | internal |
| struct | `LockedTlsf` | 143 | pub |
| fn | `new` | 154 | pub |
| fn | `is_early_heap` | 188 | internal |
| fn | `is_main_heap` | 198 | internal |
| fn | `is_valid_heap_ptr` | 209 | internal |
| fn | `check_integrity` | 220 | pub |
| fn | `corruption_count` | 251 | pub |
| fn | `check_heap_integrity` | 256 | pub |
| fn | `get_stats` | 265 | pub |
| fn | `memory_stats` | 281 | pub |
| struct | `IntegrityReport` | 321 | pub |
| struct | `MemoryStats` | 330 | pub |
| struct | `AllocStats` | 339 | pub |
| fn | `early_alloc` | 372 | internal |
| fn | `heap_stats` | 533 | pub |
| fn | `early_heap_usage` | 543 | pub |
| fn | `main_heap_bounds` | 550 | pub |

### Muhendislik notu

Bu dosyada API yuzeyi ve internal yardimci fonksiyonlarin dagilimi, kodun sahiplik ve publication sinirlarini yorumlamak icin ilk sinyal katmanidir. Sembol yogunlugu tek basina kalite olcutu degildir; ancak degisen release'lerde kontrat yuzeyinin hangi dosyada buyudugunu sayisal olarak izlemeyi mumkun kilar.

---

## src/memory/mod.rs

- Satir sayisi: 5272
- Toplam sembol: 255
- Public sembol: 114
- Fonksiyon: 192, Struct: 24, Enum: 7, Const: 32

### Sembol envanteri

| Kind | Name | Line | Visibility |
|---|---|---:|---|
| struct | `MemoryStats` | 183 | pub |
| const | `KERNEL_HEAP_BASE` | 199 | pub |
| const | `KERNEL_HEAP_SIZE` | 201 | pub |
| fn | `get_memory_stats` | 206 | pub |
| struct | `MemoryManager` | 254 | pub |
| fn | `new` | 266 | pub |
| fn | `get_memory_map` | 277 | pub |
| fn | `memory_map_mut` | 281 | pub |
| fn | `allocate_contiguous_frames` | 285 | pub |
| fn | `deallocate_contiguous_frames` | 289 | pub |
| fn | `total_frames` | 300 | pub |
| fn | `free_frames` | 304 | pub |
| fn | `allocate_frame` | 312 | internal |
| fn | `init_uefi` | 410 | pub |
| fn | `init_memory_subsystems` | 417 | pub |
| fn | `global_memory_manager` | 471 | pub |
| const | `PAGE_SIZE` | 498 | pub |
| const | `PHYSICAL_MEMORY_OFFSET` | 499 | pub |
| const | `KERNEL_SPACE_START` | 500 | pub |
| const | `KERNEL_STACK_VIRT_BASE` | 501 | pub |
| const | `KERNEL_STACK_VIRT_LIMIT` | 502 | pub |
| const | `USER_SPACE_START` | 503 | pub |
| const | `USER_SPACE_END` | 504 | pub |
| const | `USER_STACK_TOP` | 505 | pub |
| const | `USER_STACK_PAGES` | 506 | pub |
| const | `USER_HEAP_BASE` | 507 | pub |
| const | `USER_MMAP_BASE` | 508 | pub |
| const | `USER_MMAP_RANDOM_RANGE` | 509 | pub |
| const | `USER_STACK_RANDOM_RANGE` | 510 | pub |
| const | `USER_HEAP_RANDOM_RANGE` | 511 | pub |
| const | `RECLAIM_HIGH_DIV` | 520 | internal |
| const | `RECLAIM_LOW_DIV` | 521 | internal |
| const | `RECLAIM_MIN_HIGH` | 522 | internal |
| const | `RECLAIM_MIN_LOW` | 523 | internal |
| const | `KSWAPD_SLEEP_TICKS` | 524 | internal |
| const | `KSWAPD_RECLAIM_BATCH` | 525 | internal |
| const | `WRITEBACK_BUDGET_FAST` | 526 | internal |
| const | `WRITEBACK_BUDGET_IDLE` | 527 | internal |
| const | `DIRTY_BG_DIV` | 528 | internal |
| const | `DIRTY_LIMIT_DIV` | 529 | internal |
| const | `DIRTY_INODE_LIMIT` | 530 | internal |
| const | `WRITEBACK_TOKENS_PER_TICK` | 531 | internal |
| const | `WRITEBACK_INODE_TOKENS_PER_TICK` | 532 | internal |
| const | `WRITEBACK_TOKEN_CAP` | 533 | internal |
| const | `WRITEBACK_INODE_TOKEN_CAP` | 534 | internal |
| const | `THP_PAGES` | 535 | internal |
| enum | `VmaKind` | 539 | internal |
| struct | `Vma` | 556 | internal |
| struct | `ImageRef` | 566 | internal |
| struct | `PageCacheEntry` | 572 | internal |
| struct | `FrameRefCounts` | 577 | internal |
| struct | `SharedAnonPages` | 581 | internal |
| struct | `SharedFilePages` | 585 | internal |
| struct | `PageCache` | 589 | internal |
| enum | `PageBacking` | 595 | internal |
| struct | `LruEntry` | 615 | internal |
| enum | `LruClass` | 625 | internal |
| struct | `SpaceLruCounts` | 631 | internal |
| struct | `LruState` | 636 | internal |
| fn | `new` | 649 | internal |
| fn | `touch` | 663 | internal |
| fn | `pop_oldest_balanced` | 696 | internal |
| fn | `remove_page` | 719 | internal |
| fn | `record_refault` | 732 | internal |
| fn | `pop_matching` | 740 | internal |
| fn | `class_of` | 797 | internal |
| fn | `adjust_counts` | 804 | internal |
| struct | `SwapState` | 859 | internal |
| struct | `SwapDeviceState` | 863 | internal |
| struct | `WritebackEntry` | 871 | internal |
| struct | `WritebackQueue` | 879 | internal |
| struct | `DirtyThrottleState` | 885 | internal |
| fn | `new` | 894 | internal |
| fn | `insert` | 900 | internal |
| fn | `take` | 904 | internal |
| fn | `remove` | 908 | internal |
| fn | `new` | 914 | internal |
| fn | `sector_per_page` | 924 | internal |
| fn | `store` | 928 | internal |
| fn | `take` | 963 | internal |
| fn | `remove` | 982 | internal |
| fn | `new` | 988 | internal |
| fn | `push` | 996 | internal |
| fn | `pop` | 1004 | internal |
| fn | `new` | 1024 | internal |
| fn | `update_tokens` | 1034 | internal |
| fn | `consume_token` | 1053 | internal |
| fn | `mark_dirty` | 1070 | internal |
| fn | `mark_clean` | 1079 | internal |
| fn | `inode_dirty` | 1089 | internal |
| fn | `new` | 1095 | internal |
| fn | `insert` | 1102 | internal |
| fn | `frame_key` | 1139 | internal |
| fn | `frame_refcount` | 1143 | internal |
| fn | `inc_frame_ref` | 1153 | internal |
| fn | `dec_frame_ref` | 1160 | internal |
| fn | `free_frame_if_unused` | 1177 | internal |
| fn | `current_space_id` | 1184 | internal |
| fn | `register_lru_mapping` | 1188 | internal |
| fn | `remove_lru_mapping` | 1244 | internal |
| fn | `swap_take_page` | 1250 | internal |
| fn | `swap_remove_page` | 1259 | internal |
| fn | `swap_store_page` | 1266 | internal |
| fn | `memory_total_frames` | 1283 | internal |
| fn | `memory_free_frames` | 1289 | internal |
| fn | `memory_watermarks` | 1295 | internal |
| fn | `dirty_limits` | 1309 | internal |
| fn | `mark_cache_dirty` | 1325 | internal |
| fn | `mark_cache_clean` | 1339 | internal |
| fn | `maybe_throttle_dirty` | 1352 | internal |
| fn | `node_id_for_phys` | 1372 | internal |
| fn | `current_numa_node` | 1376 | internal |
| fn | `compact_memory_for_thp` | 1380 | internal |
| fn | `allocate_contiguous_huge_frame` | 1385 | internal |
| fn | `try_map_thp_anon` | 1403 | internal |
| fn | `select_reclaim_class` | 1469 | internal |
| fn | `reclaim_class_for_space` | 1484 | internal |
| fn | `reclaim_class_global` | 1493 | internal |
| fn | `should_reclaim_now` | 1498 | internal |
| fn | `should_reclaim_background` | 1503 | internal |
| fn | `page_table_flags` | 1508 | internal |
| fn | `page_is_dirty` | 1553 | internal |
| fn | `next_shared_anon_id` | 1565 | internal |
| fn | `next_address_space_id` | 1571 | internal |
| struct | `AddressSpace` | 1578 | pub |
| fn | `image_ref_from_slice` | 1590 | internal |
| fn | `image_ref_from_owned` | 1598 | internal |
| fn | `try_merge_vma` | 1620 | internal |
| fn | `merge_adjacent` | 1683 | internal |
| fn | `insert_vma` | 1700 | internal |
| fn | `active_physical_offset` | 1754 | pub |
| fn | `set_active_physical_offset` | 1758 | pub |
| fn | `set_kaslr_offset` | 1764 | pub |
| fn | `kaslr_offset` | 1768 | pub |
| fn | `kernel_virtual_base` | 1772 | pub |
| fn | `is_user_address` | 1776 | pub |
| fn | `is_user_range` | 1780 | pub |
| fn | `is_kernel_address` | 1788 | pub |
| fn | `create_address_space` | 1792 | pub |
| fn | `create_address_space_owned` | 1806 | pub |
| fn | `create_empty_address_space` | 1820 | pub |
| fn | `address_space_id` | 1824 | pub |
| fn | `allocate_user_mmap_in` | 1828 | pub |
| fn | `register_shared_anon_region_in` | 1850 | pub |
| fn | `clone_address_space_for_cow` | 1888 | pub |
| fn | `clone_user_pml4_for_cow` | 1905 | pub |
| fn | `set_active_address_space` | 1967 | pub |
| fn | `apply_cow_write_protect_current` | 1971 | pub |
| fn | `register_lazy_region` | 2013 | pub |
| fn | `register_shared_anon_region` | 2045 | pub |
| fn | `ensure_mmap_base` | 2078 | internal |
| fn | `ensure_stack_bounds` | 2091 | internal |
| fn | `user_stack_bounds` | 2118 | pub |
| fn | `user_heap_limit` | 2127 | pub |
| fn | `allocate_user_mmap` | 2134 | pub |
| fn | `update_user_region_flags` | 2157 | pub |
| fn | `initial_heap_base` | 2209 | internal |
| fn | `user_heap_state` | 2230 | pub |
| fn | `set_user_heap_break` | 2242 | pub |
| fn | `inode_key` | 2248 | internal |
| fn | `read_file_page` | 2252 | internal |
| fn | `read_cached_file_page` | 2273 | internal |
| fn | `writeback_file_range` | 2294 | internal |
| fn | `writeback_file_page` | 2352 | internal |
| fn | `schedule_writeback` | 2379 | internal |
| fn | `process_writeback_budget` | 2398 | internal |
| fn | `init_swap_device` | 2422 | pub |
| fn | `start_reclaim_daemon` | 2438 | pub |
| fn | `memory_reclaim_daemon` | 2447 | internal |
| fn | `reclaim_pages_scoped` | 2464 | internal |
| fn | `reclaim_pages` | 2646 | pub |
| fn | `reclaim_pages_global` | 2650 | pub |
| fn | `unmap_user_range` | 2654 | pub |
| fn | `register_cow_region` | 2775 | pub |
| fn | `set_user_image` | 2829 | pub |
| fn | `set_user_image_owned` | 2835 | pub |
| fn | `register_file_lazy_region` | 2841 | pub |
| fn | `register_file_backed_region` | 2881 | pub |
| fn | `user_region_overlaps` | 2926 | pub |
| fn | `user_stack_guards_region` | 2950 | pub |
| fn | `user_heap_guards_region` | 2971 | pub |
| fn | `handle_user_page_fault` | 2992 | pub |
| fn | `handle_lazy_fault` | 3007 | internal |
| fn | `enforce_wx` | 3025 | internal |
| fn | `sanitize_user_map_flags` | 3033 | internal |
| fn | `vma_map_flags` | 3041 | internal |
| fn | `update_page_flags_with_split` | 3049 | internal |
| fn | `audit_user_mappings` | 3086 | pub |
| fn | `audit_kernel_user_flags` | 3111 | pub |
| fn | `audit_page_table_security` | 3123 | pub |
| fn | `handle_anon_lazy_fault` | 3127 | internal |
| fn | `handle_image_lazy_fault` | 3273 | internal |
| fn | `handle_file_lazy_fault` | 3344 | internal |
| fn | `handle_cow_fault` | 3520 | internal |
| fn | `allocate_contiguous_frames` | 3613 | pub |
| fn | `alloc_phys` | 3621 | pub |
| fn | `free_phys` | 3629 | pub |
| fn | `deallocate_contiguous_frames` | 3638 | pub |
| struct | `DmaMapping` | 3647 | internal |
| struct | `DeviceId` | 3655 | pub |
| struct | `IommuMapping` | 3662 | internal |
| struct | `IommuDomain` | 3669 | internal |
| struct | `IommuState` | 3675 | internal |
| fn | `dma_mapping_contains` | 3696 | internal |
| fn | `find_dma_mapping` | 3701 | internal |
| fn | `insert_dma_mapping` | 3711 | internal |
| fn | `remove_dma_mapping` | 3727 | internal |
| fn | `is_kernel_range` | 3731 | internal |
| fn | `dma_alloc` | 3740 | pub |
| fn | `dma_dealloc` | 3768 | pub |
| fn | `dma_alloc` | 3784 | pub |
| fn | `dma_dealloc` | 3806 | pub |
| fn | `dma_share` | 3823 | pub |
| fn | `dma_unshare` | 3866 | pub |
| fn | `init_iommu` | 3879 | pub |
| fn | `iommu_enabled` | 3922 | pub |
| fn | `iommu_register_device` | 3926 | pub |
| fn | `iommu_domain_for_device` | 3961 | pub |
| fn | `iommu_map_domain` | 3971 | internal |
| fn | `iommu_unmap_domain` | 4005 | internal |
| fn | `dma_alloc_for_domain` | 4017 | pub |
| fn | `dma_dealloc_for_domain` | 4027 | pub |
| fn | `dma_share_for_domain` | 4036 | pub |
| fn | `dma_unshare_for_domain` | 4050 | pub |
| fn | `phys_to_virt` | 4060 | pub |
| fn | `virt_to_phys` | 4064 | pub |
| fn | `virt_to_phys_u64` | 4075 | pub |
| fn | `virt_to_phys_va` | 4087 | pub |
| fn | `hhdm_offset` | 4106 | pub |
| fn | `alloc_zeroed_page` | 4121 | pub |
| fn | `map_physical_to_user_va` | 4146 | pub |
| fn | `unmap_user_va` | 4189 | pub |
| fn | `translate_addr` | 4218 | pub |
| fn | `is_kernel_stack_virt_addr` | 4223 | pub |
| fn | `map_kernel_stack_pages` | 4227 | pub |
| fn | `unmap_kernel_stack_pages` | 4278 | pub |
| fn | `unmap_kernel_guard_page` | 4310 | pub |
| fn | `remap_kernel_guard_page` | 4355 | pub |
| fn | `mapper_update_guard_flags` | 4436 | internal |
| fn | `create_user_pml4` | 4460 | pub |
| fn | `map_mmio` | 4492 | pub |
| enum | `MmioAllocator` | 4502 | internal |
| fn | `allocate_frame` | 4508 | internal |
| fn | `map_identity` | 4597 | pub |
| enum | `IdentityAllocator` | 4605 | internal |
| fn | `allocate_frame` | 4611 | internal |
| fn | `split_huge_page` | 4695 | internal |
| fn | `ensure_identity_mapped` | 4750 | pub |
| enum | `VmmInitError` | 4807 | pub |
| fn | `map_hhdm_range` | 4939 | internal |
| enum | `UefiHhdmError` | 5039 | pub |
| struct | `Dma32FrameAllocator` | 5048 | internal |
| fn | `allocate_frame` | 5054 | internal |
| fn | `init_uefi_hhdm` | 5062 | pub |
| fn | `set_uefi_virtual_address_map` | 5248 | pub |

### Muhendislik notu

Bu dosyada API yuzeyi ve internal yardimci fonksiyonlarin dagilimi, kodun sahiplik ve publication sinirlarini yorumlamak icin ilk sinyal katmanidir. Sembol yogunlugu tek basina kalite olcutu degildir; ancak degisen release'lerde kontrat yuzeyinin hangi dosyada buyudugunu sayisal olarak izlemeyi mumkun kilar.

---

## src/memory/mglru.rs

- Satir sayisi: 339
- Toplam sembol: 29
- Public sembol: 12
- Fonksiyon: 21, Struct: 5, Enum: 0, Const: 3

### Sembol envanteri

| Kind | Name | Line | Visibility |
|---|---|---:|---|
| const | `MGLRU_GENERATIONS` | 14 | internal |
| const | `HOT_REF_THRESHOLD` | 15 | internal |
| const | `COLD_EVICTION_AGE` | 16 | internal |
| struct | `MglruPageKey` | 19 | pub |
| struct | `MglruVictim` | 25 | pub |
| struct | `MglruEntry` | 33 | internal |
| struct | `MglruStats` | 42 | pub |
| struct | `MglruState` | 52 | internal |
| fn | `new` | 63 | internal |
| fn | `generation_slot` | 75 | internal |
| fn | `detach_from_generation` | 79 | internal |
| fn | `attach_to_generation` | 88 | internal |
| fn | `set_generation` | 93 | internal |
| fn | `on_access` | 111 | internal |
| fn | `age_tick` | 153 | internal |
| fn | `remove_page` | 174 | internal |
| fn | `record_refault` | 181 | internal |
| fn | `record_eviction` | 193 | internal |
| fn | `pick_victim` | 198 | internal |
| fn | `stats` | 236 | internal |
| fn | `init` | 256 | pub |
| fn | `is_enabled` | 260 | pub |
| fn | `record_page_access` | 264 | pub |
| fn | `age_generations` | 285 | pub |
| fn | `record_refault` | 297 | pub |
| fn | `record_eviction` | 310 | pub |
| fn | `remove_page` | 320 | pub |
| fn | `pick_victim` | 330 | pub |
| fn | `get_stats` | 337 | pub |

### Muhendislik notu

Bu dosyada API yuzeyi ve internal yardimci fonksiyonlarin dagilimi, kodun sahiplik ve publication sinirlarini yorumlamak icin ilk sinyal katmanidir. Sembol yogunlugu tek basina kalite olcutu degildir; ancak degisen release'lerde kontrat yuzeyinin hangi dosyada buyudugunu sayisal olarak izlemeyi mumkun kilar.

---

## src/memory/zswap.rs

- Satir sayisi: 730
- Toplam sembol: 50
- Public sembol: 37
- Fonksiyon: 37, Struct: 8, Enum: 1, Const: 4

### Sembol envanteri

| Kind | Name | Line | Visibility |
|---|---|---:|---|
| const | `ZSWAP_MAX_POOL_PERCENT` | 81 | pub |
| const | `ZSWAP_DEFAULT_POOL_PERCENT` | 83 | pub |
| const | `ZSWAP_MAX_ZBUD_PAGES` | 85 | pub |
| const | `ZSWAP_DEFAULT_COMPRESSOR` | 87 | pub |
| fn | `compress` | 96 | internal |
| fn | `decompress` | 98 | internal |
| fn | `name` | 100 | internal |
| struct | `Lz4Compressor` | 105 | pub |
| fn | `compress` | 108 | internal |
| fn | `decompress` | 180 | internal |
| fn | `name` | 224 | internal |
| struct | `ZstdCompressor` | 230 | pub |
| fn | `compress` | 233 | internal |
| fn | `decompress` | 239 | internal |
| fn | `name` | 243 | internal |
| struct | `ZswapEntry` | 254 | pub |
| fn | `new` | 270 | pub |
| fn | `compression_ratio` | 288 | pub |
| fn | `clone` | 297 | internal |
| struct | `ZswapPool` | 314 | pub |
| fn | `new` | 332 | pub |
| fn | `store` | 345 | pub |
| fn | `load` | 377 | pub |
| fn | `remove` | 391 | pub |
| fn | `alloc_handle` | 407 | internal |
| fn | `free_handle` | 414 | internal |
| fn | `get_data` | 419 | internal |
| fn | `compression_ratio` | 425 | pub |
| struct | `ZswapStats` | 441 | pub |
| struct | `ZswapManager` | 458 | pub |
| fn | `new` | 474 | pub |
| fn | `init` | 486 | pub |
| fn | `store` | 505 | pub |
| fn | `load` | 540 | pub |
| fn | `invalidate` | 552 | pub |
| fn | `writeback_lru` | 561 | pub |
| fn | `compression_ratio` | 569 | pub |
| fn | `get_stats` | 578 | pub |
| fn | `set_max_pool_percent` | 583 | pub |
| fn | `set_enabled` | 589 | pub |
| struct | `ZramDevice` | 604 | pub |
| struct | `ZramStats` | 618 | pub |
| fn | `new` | 638 | pub |
| fn | `write` | 649 | pub |
| fn | `read` | 675 | pub |
| fn | `set_size` | 688 | pub |
| fn | `reset` | 694 | pub |
| enum | `ZswapError` | 706 | pub |
| fn | `init` | 722 | pub |
| fn | `is_enabled` | 728 | pub |

### Muhendislik notu

Bu dosyada API yuzeyi ve internal yardimci fonksiyonlarin dagilimi, kodun sahiplik ve publication sinirlarini yorumlamak icin ilk sinyal katmanidir. Sembol yogunlugu tek basina kalite olcutu degildir; ancak degisen release'lerde kontrat yuzeyinin hangi dosyada buyudugunu sayisal olarak izlemeyi mumkun kilar.

---

## src/posix/io_uring_ring.rs

- Satir sayisi: 859
- Toplam sembol: 49
- Public sembol: 40
- Fonksiyon: 31, Struct: 6, Enum: 0, Const: 12

### Sembol envanteri

| Kind | Name | Line | Visibility |
|---|---|---:|---|
| struct | `SendPtr` | 47 | pub |
| fn | `new` | 53 | pub |
| fn | `as_ptr` | 57 | pub |
| const | `RING_SIZE` | 64 | internal |
| const | `RING_MASK` | 67 | internal |
| struct | `RingSqe` | 79 | pub |
| fn | `default` | 109 | internal |
| struct | `RingCqe` | 134 | pub |
| fn | `default` | 144 | internal |
| const | `IORING_OP_NOP` | 158 | pub |
| const | `IORING_OP_READV` | 160 | pub |
| const | `IORING_OP_WRITEV` | 162 | pub |
| const | `IORING_OP_FSYNC` | 164 | pub |
| const | `IORING_OP_READ_FIXED` | 166 | pub |
| const | `IORING_OP_WRITE_FIXED` | 168 | pub |
| const | `IORING_OP_POLL_ADD` | 170 | pub |
| const | `IORING_OP_POLL_REMOVE` | 172 | pub |
| const | `IORING_OP_READ` | 174 | pub |
| const | `IORING_OP_WRITE` | 176 | pub |
| struct | `SubmissionRing` | 195 | pub |
| struct | `CompletionRing` | 215 | pub |
| fn | `new` | 232 | pub |
| fn | `pending_count` | 261 | pub |
| fn | `is_full` | 269 | pub |
| fn | `is_empty` | 275 | pub |
| fn | `capacity` | 281 | pub |
| fn | `push` | 298 | pub |
| fn | `pop` | 338 | pub |
| fn | `pop_batch` | 371 | pub |
| fn | `new` | 403 | pub |
| fn | `pending_count` | 420 | pub |
| fn | `is_full` | 428 | pub |
| fn | `is_empty` | 434 | pub |
| fn | `push` | 447 | pub |
| fn | `pop` | 489 | pub |
| fn | `pop_batch` | 513 | pub |
| fn | `drain_overflow` | 541 | pub |
| struct | `LockFreeIoUring` | 560 | pub |
| fn | `new` | 574 | pub |
| fn | `process_submissions` | 594 | pub |
| fn | `completions_available` | 751 | pub |
| fn | `submissions_pending` | 757 | pub |
| fn | `cq_overflow_count` | 762 | pub |
| fn | `sq_dropped_count` | 767 | pub |
| fn | `test_sq_push_pop` | 781 | internal |
| fn | `test_cq_push_pop` | 800 | internal |
| fn | `test_ring_full` | 814 | internal |
| fn | `test_batch_pop` | 825 | internal |
| fn | `test_wrapping_arithmetic` | 843 | internal |

### Muhendislik notu

Bu dosyada API yuzeyi ve internal yardimci fonksiyonlarin dagilimi, kodun sahiplik ve publication sinirlarini yorumlamak icin ilk sinyal katmanidir. Sembol yogunlugu tek basina kalite olcutu degildir; ancak degisen release'lerde kontrat yuzeyinin hangi dosyada buyudugunu sayisal olarak izlemeyi mumkun kilar.

---

## src/net/tls.rs

- Satir sayisi: 4111
- Toplam sembol: 191
- Public sembol: 131
- Fonksiyon: 156, Struct: 18, Enum: 10, Const: 7

### Sembol envanteri

| Kind | Name | Line | Visibility |
|---|---|---:|---|
| const | `TLS_VERSION_1_3` | 97 | pub |
| enum | `ContentType` | 107 | pub |
| fn | `from_u8` | 115 | pub |
| enum | `HandshakeType` | 131 | pub |
| fn | `from_u8` | 145 | pub |
| enum | `CipherSuite` | 167 | pub |
| fn | `from_u16` | 174 | pub |
| fn | `key_len` | 183 | pub |
| fn | `iv_len` | 191 | pub |
| enum | `NamedGroup` | 198 | pub |
| fn | `from_u16` | 207 | pub |
| enum | `SignatureScheme` | 224 | pub |
| enum | `TlsError` | 243 | pub |
| enum | `AlertLevel` | 259 | pub |
| enum | `AlertDescription` | 266 | pub |
| enum | `TlsState` | 295 | pub |
| struct | `TlsRecordHeader` | 324 | pub |
| const | `SIZE` | 331 | pub |
| fn | `parse` | 333 | pub |
| fn | `to_bytes` | 349 | pub |
| struct | `HandshakeHeader` | 374 | pub |
| const | `SIZE` | 380 | pub |
| fn | `parse` | 382 | pub |
| fn | `to_bytes` | 393 | pub |
| struct | `KeySchedule` | 420 | pub |
| fn | `new` | 431 | pub |
| fn | `derive_handshake_secret` | 443 | pub |
| fn | `derive_master_secret` | 459 | pub |
| fn | `derive_traffic_secret` | 480 | internal |
| fn | `hkdf_expand_label` | 489 | internal |
| fn | `client_handshake_traffic_secret` | 515 | pub |
| fn | `server_handshake_traffic_secret` | 519 | pub |
| fn | `client_application_traffic_secret` | 523 | pub |
| fn | `server_application_traffic_secret` | 527 | pub |
| fn | `server_finished_verify_data` | 531 | pub |
| fn | `resumption_psk` | 539 | pub |
| fn | `default` | 551 | internal |
| struct | `TlsClient` | 580 | pub |
| fn | `new` | 600 | pub |
| fn | `build_client_hello` | 634 | pub |
| fn | `process_server_hello` | 794 | pub |
| fn | `process_encrypted_extensions` | 936 | pub |
| fn | `process_certificate` | 959 | pub |
| fn | `process_certificate_verify` | 981 | pub |
| fn | `process_finished` | 1020 | pub |
| fn | `complete_handshake` | 1054 | pub |
| fn | `process_new_session_ticket` | 1067 | pub |
| fn | `cache_new_session_ticket` | 1081 | internal |
| fn | `state` | 1099 | pub |
| fn | `is_established` | 1102 | pub |
| fn | `cipher_suite` | 1105 | pub |
| fn | `default` | 1111 | internal |
| fn | `parse_signature_scheme` | 1131 | internal |
| fn | `finished_verify_len` | 1142 | internal |
| fn | `has_tls13_downgrade_sentinel` | 1149 | internal |
| fn | `constant_time_eq` | 1153 | internal |
| fn | `build_server_certificate_verify_message` | 1165 | internal |
| fn | `parse_tls13_leaf_public_key` | 1173 | internal |
| fn | `verify_tls13_certificate_signature` | 1214 | internal |
| fn | `parse_tls_rsa_public_key_components` | 1262 | internal |
| fn | `trim_der_integer` | 1277 | internal |
| fn | `ecdsa_der_to_fixed` | 1284 | internal |
| fn | `normalize_ecdsa_integer` | 1299 | internal |
| fn | `verify_p256_certificate_signature` | 1311 | internal |
| fn | `verify_p256_certificate_signature` | 1330 | internal |
| fn | `verify_p384_certificate_signature` | 1339 | internal |
| fn | `verify_p384_certificate_signature` | 1358 | internal |
| fn | `wrap_record` | 1377 | pub |
| fn | `parse_record` | 1393 | pub |
| fn | `transcript_hash` | 1404 | pub |
| fn | `session_ticket_hash_len` | 1411 | internal |
| fn | `digest_for_cipher_suite` | 1418 | internal |
| fn | `hmac_for_cipher_suite` | 1427 | internal |
| struct | `Aes` | 1471 | pub |
| fn | `new` | 1478 | pub |
| fn | `new_aes128` | 1486 | internal |
| fn | `new_aes256` | 1518 | internal |
| fn | `rot_word` | 1550 | internal |
| fn | `sub_word` | 1554 | internal |
| fn | `sbox` | 1564 | internal |
| const | `SBOX_VALUES` | 1567 | internal |
| fn | `inv_sbox` | 1592 | internal |
| const | `INV_SBOX_VALUES` | 1594 | internal |
| fn | `encrypt_block` | 1620 | pub |
| fn | `decrypt_block` | 1660 | pub |
| fn | `sub_bytes` | 1699 | internal |
| fn | `inv_sub_bytes` | 1712 | internal |
| fn | `shift_rows` | 1725 | internal |
| fn | `inv_shift_rows` | 1734 | internal |
| fn | `mix_columns` | 1740 | internal |
| fn | `xtime` | 1741 | internal |
| fn | `mul` | 1748 | internal |
| fn | `inv_mix_columns` | 1772 | internal |
| fn | `xtime` | 1773 | internal |
| fn | `mul` | 1780 | internal |
| struct | `AesGcm` | 1812 | pub |
| fn | `new` | 1818 | pub |
| fn | `encrypt` | 1830 | pub |
| fn | `decrypt` | 1875 | pub |
| fn | `ghash` | 1921 | internal |
| fn | `gmul` | 1970 | internal |
| struct | `ChaCha20` | 2027 | pub |
| fn | `new` | 2039 | pub |
| fn | `quarter_round` | 2075 | internal |
| fn | `block` | 2098 | pub |
| fn | `process` | 2135 | pub |
| struct | `Poly1305` | 2177 | pub |
| fn | `new` | 2197 | pub |
| fn | `process_block` | 2242 | internal |
| fn | `update` | 2315 | pub |
| fn | `finalize` | 2354 | pub |
| struct | `ChaCha20Poly1305` | 2436 | pub |
| fn | `new` | 2441 | pub |
| fn | `encrypt` | 2453 | pub |
| fn | `decrypt` | 2477 | pub |
| struct | `FieldElement` | 2540 | pub |
| const | `P` | 2544 | internal |
| fn | `zero` | 2553 | pub |
| fn | `one` | 2558 | pub |
| fn | `from_bytes` | 2563 | pub |
| fn | `to_bytes` | 2612 | pub |
| fn | `add` | 2673 | pub |
| fn | `sub` | 2682 | pub |
| fn | `mul` | 2693 | pub |
| fn | `square` | 2722 | pub |
| fn | `reduce` | 2727 | pub |
| fn | `invert` | 2746 | pub |
| fn | `conditional_swap` | 2768 | pub |
| struct | `X25519` | 2786 | pub |
| const | `A24` | 2790 | internal |
| fn | `generate_keypair` | 2793 | pub |
| fn | `public_from_private` | 2807 | pub |
| fn | `scalar_mult` | 2814 | pub |
| fn | `diffie_hellman` | 2895 | pub |
| struct | `TlsKeySchedule` | 2926 | pub |
| fn | `new` | 2949 | pub |
| fn | `hkdf_extract` | 2973 | internal |
| fn | `hkdf_expand` | 2997 | internal |
| fn | `hmac_hash` | 3032 | internal |
| fn | `hkdf_expand_label` | 3087 | pub |
| fn | `derive_secret` | 3113 | pub |
| fn | `init_with_psk` | 3118 | pub |
| fn | `derive_handshake_secrets` | 3127 | pub |
| fn | `derive_master_secret` | 3144 | pub |
| fn | `derive_traffic_keys` | 3166 | pub |
| fn | `client_hs_secret` | 3183 | pub |
| fn | `server_hs_secret` | 3188 | pub |
| fn | `client_app_secret` | 3193 | pub |
| fn | `server_app_secret` | 3198 | pub |
| fn | `compute_finished_mac` | 3205 | pub |
| fn | `update_traffic_secret` | 3211 | pub |
| struct | `TlsCrypto` | 3217 | pub |
| fn | `new` | 3224 | pub |
| fn | `encrypt_record` | 3233 | pub |
| fn | `decrypt_record` | 3267 | pub |
| struct | `EarlyDataConfig` | 3334 | pub |
| fn | `default` | 3342 | internal |
| struct | `SessionTicket` | 3352 | pub |
| fn | `new` | 3375 | pub |
| fn | `is_valid` | 3394 | pub |
| fn | `obfuscated_age` | 3401 | pub |
| fn | `derive_early_secret` | 3413 | pub |
| enum | `EarlyDataState` | 3449 | pub |
| struct | `ZeroRttState` | 3462 | pub |
| fn | `new` | 3477 | pub |
| fn | `with_ticket` | 3488 | pub |
| fn | `can_send_early_data` | 3500 | pub |
| fn | `send_early_data` | 3509 | pub |
| fn | `on_reject` | 3532 | pub |
| fn | `on_accept` | 3539 | pub |
| fn | `get_retry_data` | 3544 | pub |
| fn | `default` | 3550 | internal |
| struct | `SessionCache` | 3586 | pub |
| fn | `new` | 3592 | pub |
| fn | `add` | 3600 | pub |
| fn | `find_for_server` | 3613 | pub |
| fn | `remove` | 3623 | pub |
| fn | `clear` | 3628 | pub |
| fn | `default` | 3634 | internal |
| struct | `TlsHandshakeExt` | 3673 | pub |
| fn | `new` | 3691 | pub |
| fn | `start_with_early_data` | 3704 | pub |
| fn | `build_client_hello` | 3721 | internal |
| fn | `process_server_response` | 3852 | pub |
| fn | `send_data` | 3925 | pub |
| fn | `parse_tls_handshake_body` | 3939 | internal |
| fn | `parse_server_hello_psk_selection` | 3950 | internal |
| fn | `encrypted_extensions_has_early_data` | 3991 | internal |
| fn | `extract_new_session_ticket_nonce` | 4021 | internal |
| fn | `parse_new_session_ticket` | 4033 | internal |
| fn | `default` | 4108 | internal |

### Muhendislik notu

Bu dosyada API yuzeyi ve internal yardimci fonksiyonlarin dagilimi, kodun sahiplik ve publication sinirlarini yorumlamak icin ilk sinyal katmanidir. Sembol yogunlugu tek basina kalite olcutu degildir; ancak degisen release'lerde kontrat yuzeyinin hangi dosyada buyudugunu sayisal olarak izlemeyi mumkun kilar.

---

## src/net/quic.rs

- Satir sayisi: 2311
- Toplam sembol: 81
- Public sembol: 62
- Fonksiyon: 61, Struct: 8, Enum: 9, Const: 3

### Sembol envanteri

| Kind | Name | Line | Visibility |
|---|---|---:|---|
| const | `QUIC_VERSION_1` | 103 | pub |
| const | `MAX_ACK_RANGES` | 104 | internal |
| enum | `QuicPacketType` | 109 | pub |
| enum | `QuicFrameType` | 125 | pub |
| enum | `QuicError` | 175 | pub |
| struct | `ConnectionId` | 225 | pub |
| fn | `new` | 232 | pub |
| fn | `random` | 238 | pub |
| fn | `len` | 247 | pub |
| fn | `is_empty` | 252 | pub |
| fn | `as_slice` | 257 | pub |
| enum | `StreamType` | 285 | pub |
| enum | `StreamState` | 298 | pub |
| struct | `QuicStream` | 317 | pub |
| fn | `new` | 345 | pub |
| fn | `can_read` | 362 | pub |
| fn | `can_write` | 368 | pub |
| fn | `write` | 377 | pub |
| fn | `read` | 393 | pub |
| enum | `QuicFrame` | 424 | pub |
| fn | `encode` | 508 | pub |
| fn | `encode_varint` | 656 | internal |
| fn | `decode_varint` | 681 | internal |
| fn | `decode` | 712 | pub |
| enum | `QuicState` | 838 | pub |
| enum | `QuicCryptoLevel` | 859 | pub |
| struct | `QuicKeys` | 870 | pub |
| struct | `QuicConnection` | 883 | pub |
| fn | `new` | 934 | pub |
| fn | `create_stream` | 963 | pub |
| fn | `get_stream` | 976 | pub |
| fn | `get_stream_mut` | 981 | pub |
| fn | `on_packet` | 986 | pub |
| fn | `parse_frames` | 1034 | internal |
| fn | `build_packet` | 1092 | pub |
| fn | `encode_varint` | 1143 | internal |
| struct | `QuicClient` | 1166 | pub |
| fn | `new` | 1172 | pub |
| fn | `connect` | 1180 | pub |
| fn | `send` | 1245 | pub |
| fn | `create_stream` | 1267 | pub |
| struct | `QuicServer` | 1277 | pub |
| fn | `new` | 1282 | pub |
| fn | `on_packet` | 1289 | pub |
| fn | `created_stream_starts_open_with_send_window` | 1393 | internal |
| fn | `decode_rejects_ack_with_excessive_ranges` | 1407 | internal |
| fn | `default` | 1421 | internal |
| fn | `compute_nonce` | 1431 | pub |
| fn | `compute_header_protection_mask` | 1448 | pub |
| fn | `aes_key_expansion` | 1472 | internal |
| fn | `aes_encrypt_block` | 1505 | internal |
| fn | `gf_mul2` | 1588 | internal |
| fn | `gf_mul3` | 1597 | internal |
| fn | `protect_long_header` | 1602 | pub |
| fn | `unprotect_long_header` | 1627 | pub |
| fn | `encrypt_packet_payload` | 1650 | pub |
| fn | `decrypt_packet_payload` | 1684 | pub |
| struct | `SentPacket` | 1719 | pub |
| struct | `LossRecovery` | 1730 | pub |
| enum | `CongestionState` | 1779 | pub |
| fn | `new` | 1787 | pub |
| fn | `on_packet_sent` | 1813 | pub |
| fn | `on_ack_received` | 1839 | pub |
| fn | `update_rtt` | 1878 | pub |
| fn | `detect_lost_packets` | 1910 | pub |
| fn | `pto` | 1968 | pub |
| fn | `loss_detection_timeout` | 1977 | pub |
| fn | `earliest_loss_time` | 1994 | internal |
| fn | `on_pto_expired` | 2007 | pub |
| fn | `on_packets_acked` | 2018 | internal |
| fn | `on_congestion_event` | 2065 | internal |
| fn | `can_send` | 2073 | pub |
| fn | `send_window` | 2078 | pub |
| fn | `default` | 2084 | internal |
| fn | `derive_initial_secret` | 2094 | pub |
| fn | `hkdf_extract` | 2131 | internal |
| fn | `hkdf_expand` | 2140 | internal |
| fn | `hmac_sha256` | 2165 | pub |
| fn | `sha256_hash` | 2206 | pub |
| const | `K` | 2208 | internal |
| fn | `update_key` | 2299 | pub |

### Muhendislik notu

Bu dosyada API yuzeyi ve internal yardimci fonksiyonlarin dagilimi, kodun sahiplik ve publication sinirlarini yorumlamak icin ilk sinyal katmanidir. Sembol yogunlugu tek basina kalite olcutu degildir; ancak degisen release'lerde kontrat yuzeyinin hangi dosyada buyudugunu sayisal olarak izlemeyi mumkun kilar.

---

## src/net/wireguard.rs

- Satir sayisi: 808
- Toplam sembol: 49
- Public sembol: 36
- Fonksiyon: 32, Struct: 7, Enum: 1, Const: 9

### Sembol envanteri

| Kind | Name | Line | Visibility |
|---|---|---:|---|
| const | `WG_DEFAULT_PORT` | 58 | pub |
| const | `WG_KEY_SIZE` | 61 | pub |
| const | `WG_MSG_INITIATION` | 64 | pub |
| const | `WG_MSG_RESPONSE` | 66 | pub |
| const | `WG_MSG_COOKIE_REPLY` | 68 | pub |
| const | `WG_MSG_TRANSPORT` | 70 | pub |
| const | `WG_TRANSPORT_HEADER_LEN` | 73 | internal |
| const | `WG_TRANSPORT_TAG_LEN` | 75 | internal |
| const | `WG_NONCE_UNINITIALIZED` | 77 | internal |
| struct | `WgKey` | 88 | pub |
| fn | `new` | 92 | pub |
| fn | `from_bytes` | 97 | pub |
| fn | `generate` | 102 | pub |
| fn | `as_bytes` | 113 | pub |
| struct | `WgPeer` | 127 | pub |
| fn | `clone` | 153 | internal |
| struct | `WgSession` | 174 | pub |
| fn | `new` | 199 | pub |
| fn | `is_allowed_ip` | 229 | pub |
| fn | `encrypt_packet` | 253 | pub |
| fn | `decrypt_packet` | 297 | pub |
| struct | `WgDevice` | 361 | pub |
| struct | `WgStats` | 382 | pub |
| fn | `new` | 393 | pub |
| fn | `add_peer` | 412 | pub |
| fn | `remove_peer` | 420 | pub |
| fn | `get_peer` | 425 | pub |
| fn | `find_peer_by_ip` | 430 | pub |
| fn | `select_single_handshake_peer` | 439 | internal |
| fn | `initiate_handshake` | 451 | pub |
| fn | `process_message` | 473 | pub |
| fn | `process_initiation` | 499 | internal |
| fn | `process_response` | 570 | internal |
| fn | `process_cookie_reply` | 619 | internal |
| fn | `process_transport` | 634 | internal |
| fn | `send_keepalive` | 660 | pub |
| fn | `rand_u32` | 670 | internal |
| fn | `generate_x25519_private` | 674 | internal |
| fn | `derive_handshake_transport_keys` | 680 | internal |
| fn | `derive_transport_key` | 690 | internal |
| struct | `WgManager` | 714 | pub |
| fn | `new` | 721 | pub |
| fn | `create_device` | 728 | pub |
| fn | `delete_device` | 739 | pub |
| fn | `get_device` | 744 | pub |
| struct | `WgRuntimeStatus` | 754 | pub |
| fn | `runtime_status` | 760 | pub |
| enum | `WgError` | 786 | pub |
| fn | `init` | 806 | pub |

### Muhendislik notu

Bu dosyada API yuzeyi ve internal yardimci fonksiyonlarin dagilimi, kodun sahiplik ve publication sinirlarini yorumlamak icin ilk sinyal katmanidir. Sembol yogunlugu tek basina kalite olcutu degildir; ancak degisen release'lerde kontrat yuzeyinin hangi dosyada buyudugunu sayisal olarak izlemeyi mumkun kilar.

---

## src/net/http2_huffman.rs

- Satir sayisi: 406
- Toplam sembol: 12
- Public sembol: 2
- Fonksiyon: 7, Struct: 2, Enum: 2, Const: 0

### Sembol envanteri

| Kind | Name | Line | Visibility |
|---|---|---:|---|
| enum | `HuffmanDecodeError` | 5 | pub |
| enum | `HuffmanCodeSymbol` | 11 | internal |
| fn | `new` | 17 | internal |
| struct | `HuffmanDecoder` | 26 | internal |
| fn | `from_table` | 32 | internal |
| fn | `new` | 52 | internal |
| fn | `decode` | 56 | internal |
| struct | `BitIterator` | 106 | internal |
| fn | `new` | 113 | internal |
| type | `Item` | 123 | internal |
| fn | `next` | 125 | internal |
| fn | `decode_huffman` | 144 | pub |

### Muhendislik notu

Bu dosyada API yuzeyi ve internal yardimci fonksiyonlarin dagilimi, kodun sahiplik ve publication sinirlarini yorumlamak icin ilk sinyal katmanidir. Sembol yogunlugu tek basina kalite olcutu degildir; ancak degisen release'lerde kontrat yuzeyinin hangi dosyada buyudugunu sayisal olarak izlemeyi mumkun kilar.

---
