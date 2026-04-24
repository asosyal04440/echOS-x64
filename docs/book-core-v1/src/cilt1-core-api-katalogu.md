# Cilt 1 Core API Katalogu

Bu katalog, core ciltte kullandigimiz dosyalardaki public API yuzeyini tek tabloda toplar.

## A01 - Boot, platform init ve erken dogruluk

- Dosya: `src/main.rs`
- Public sembol sayisi: 0

| Tip | Sembol | Konum |
|---|---|---|

## A02 - Bootstrap frame allocator ve fiziksel aralik korumasi

- Dosya: `src/memory/frame_allocator.rs`
- Public sembol sayisi: 8

| Tip | Sembol | Konum |
|---|---|---|
| struct | `Multiboot2FrameAllocator` | `src/memory/frame_allocator.rs:65` |
| fn | `new` | `src/memory/frame_allocator.rs:75` |
| fn | `total_usable_bytes` | `src/memory/frame_allocator.rs:113` |
| fn | `allocate_contiguous` | `src/memory/frame_allocator.rs:117` |
| struct | `LimineMemmapEntry` | `src/memory/frame_allocator.rs:225` |
| struct | `LimineFrameAllocator` | `src/memory/frame_allocator.rs:231` |
| fn | `new` | `src/memory/frame_allocator.rs:241` |
| fn | `total_usable_bytes` | `src/memory/frame_allocator.rs:269` |

## A03 - SMP scheduler karar modeli

- Dosya: `src/task/scheduler.rs`
- Public sembol sayisi: 68

| Tip | Sembol | Konum |
|---|---|---|
| struct | `SmpScheduler` | `src/task/scheduler.rs:80` |
| fn | `new` | `src/task/scheduler.rs:85` |
| fn | `allocate_task_id` | `src/task/scheduler.rs:91` |
| fn | `spawn` | `src/task/scheduler.rs:95` |
| fn | `spawn_boxed` | `src/task/scheduler.rs:106` |
| fn | `queued_task_count` | `src/task/scheduler.rs:130` |
| fn | `init` | `src/task/scheduler.rs:233` |
| fn | `get_cpu_load` | `src/task/scheduler.rs:282` |
| fn | `update_cpu_count` | `src/task/scheduler.rs:296` |
| fn | `enable_secondary_scheduling` | `src/task/scheduler.rs:318` |
| fn | `secondary_scheduling_active` | `src/task/scheduler.rs:322` |
| fn | `current_kernel_stack_top` | `src/task/scheduler.rs:326` |
| fn | `classify_current_kernel_stack_fault` | `src/task/scheduler.rs:343` |
| fn | `record_current_stack_pointer` | `src/task/scheduler.rs:357` |
| fn | `current_kernel_stack_usage` | `src/task/scheduler.rs:374` |
| fn | `current_user_target` | `src/task/scheduler.rs:392` |
| fn | `current_win32_thread_state` | `src/task/scheduler.rs:407` |
| fn | `current_task_id` | `src/task/scheduler.rs:427` |
| fn | `current_execution_mode` | `src/task/scheduler.rs:438` |
| fn | `current_user_page_table` | `src/task/scheduler.rs:448` |
| fn | `current_address_space` | `src/task/scheduler.rs:458` |
| fn | `task_exists` | `src/task/scheduler.rs:468` |
| fn | `fork_current_user_task` | `src/task/scheduler.rs:486` |
| fn | `idle_loop` | `src/task/scheduler.rs:522` |
| fn | `spawn` | `src/task/scheduler.rs:542` |
| fn | `spawn_with_priority` | `src/task/scheduler.rs:547` |
| fn | `spawn_with_priority_in_address_space` | `src/task/scheduler.rs:569` |
| fn | `get_ticks` | `src/task/scheduler.rs:594` |
| fn | `is_ready` | `src/task/scheduler.rs:598` |
| fn | `tick` | `src/task/scheduler.rs:606` |
| fn | `sleep` | `src/task/scheduler.rs:735` |
| fn | `exit` | `src/task/scheduler.rs:754` |
| fn | `wait_for_terminated` | `src/task/scheduler.rs:782` |
| fn | `get_current_ptrace_flags` | `src/task/scheduler.rs:803` |
| fn | `set_ptrace_flag` | `src/task/scheduler.rs:817` |
| fn | `get_current_seccomp_mode` | `src/task/scheduler.rs:830` |
| fn | `set_current_seccomp_mode` | `src/task/scheduler.rs:844` |
| fn | `exec_current_user_image` | `src/task/scheduler.rs:854` |
| fn | `spawn_user_image_task` | `src/task/scheduler.rs:890` |
| fn | `spawn_user_image_task_with_address_space` | `src/task/scheduler.rs:898` |
| fn | `schedule` | `src/task/scheduler.rs:1127` |
| struct | `TaskInfo` | `src/task/scheduler.rs:1550` |
| fn | `list_tasks` | `src/task/scheduler.rs:1559` |
| fn | `kill_task` | `src/task/scheduler.rs:1594` |
| fn | `stop_task` | `src/task/scheduler.rs:1643` |
| fn | `continue_task` | `src/task/scheduler.rs:1648` |
| fn | `background_current` | `src/task/scheduler.rs:1653` |
| fn | `get_foreground_task` | `src/task/scheduler.rs:1668` |
| fn | `foreground_task` | `src/task/scheduler.rs:1687` |
| fn | `get_task_state` | `src/task/scheduler.rs:1709` |
| fn | `get_cpu_count` | `src/task/scheduler.rs:1723` |
| fn | `steal_from_cpu` | `src/task/scheduler.rs:1729` |
| fn | `push_to_cpu` | `src/task/scheduler.rs:1741` |
| struct | `SchedulerStats` | `src/task/scheduler.rs:1754` |
| fn | `get_stats` | `src/task/scheduler.rs:1762` |
| fn | `process_deferred_timers` | `src/task/scheduler.rs:1794` |
| struct | `WaitQueue` | `src/task/scheduler.rs:1816` |
| const | `fn` | `src/task/scheduler.rs:1821` |
| fn | `sleep` | `src/task/scheduler.rs:1830` |
| fn | `wake_one` | `src/task/scheduler.rs:1849` |
| fn | `wake_all` | `src/task/scheduler.rs:1862` |
| fn | `waiter_count` | `src/task/scheduler.rs:1873` |
| fn | `has_waiters` | `src/task/scheduler.rs:1878` |
| fn | `take_current_blocked_task` | `src/task/scheduler.rs:1885` |
| fn | `wake_blocked_task` | `src/task/scheduler.rs:1898` |
| fn | `spawn_task` | `src/task/scheduler.rs:1905` |
| fn | `block_current_task` | `src/task/scheduler.rs:1913` |
| fn | `unblock_task` | `src/task/scheduler.rs:1930` |

## A04 - RT scheduler: FIFO/RR ve runtime limiti

- Dosya: `src/task/rt_scheduler.rs`
- Public sembol sayisi: 49

| Tip | Sembol | Konum |
|---|---|---|
| const | `RT_PRIO_MIN` | `src/task/rt_scheduler.rs:59` |
| const | `RT_PRIO_MAX` | `src/task/rt_scheduler.rs:62` |
| const | `RR_DEFAULT_TIMESLICE` | `src/task/rt_scheduler.rs:66` |
| const | `RR_MAX_TIMESLICE` | `src/task/rt_scheduler.rs:69` |
| const | `RR_MIN_TIMESLICE` | `src/task/rt_scheduler.rs:72` |
| enum | `SchedPolicy` | `src/task/rt_scheduler.rs:81` |
| struct | `RtSchedParam` | `src/task/rt_scheduler.rs:104` |
| struct | `RtTaskInfo` | `src/task/rt_scheduler.rs:132` |
| fn | `new` | `src/task/rt_scheduler.rs:147` |
| fn | `with_rt` | `src/task/rt_scheduler.rs:159` |
| fn | `reset_timeslice` | `src/task/rt_scheduler.rs:188` |
| fn | `tick` | `src/task/rt_scheduler.rs:194` |
| struct | `RtRunQueue` | `src/task/rt_scheduler.rs:214` |
| fn | `new` | `src/task/rt_scheduler.rs:231` |
| fn | `enqueue` | `src/task/rt_scheduler.rs:244` |
| fn | `dequeue` | `src/task/rt_scheduler.rs:269` |
| fn | `pick_next` | `src/task/rt_scheduler.rs:295` |
| fn | `rt_task_count` | `src/task/rt_scheduler.rs:340` |
| fn | `has_rt_tasks` | `src/task/rt_scheduler.rs:345` |
| fn | `set_sched_param` | `src/task/rt_scheduler.rs:350` |
| fn | `get_sched_param` | `src/task/rt_scheduler.rs:379` |
| fn | `tick` | `src/task/rt_scheduler.rs:395` |
| fn | `reenqueue_rr` | `src/task/rt_scheduler.rs:406` |
| fn | `set_rt_bandwidth` | `src/task/rt_scheduler.rs:417` |
| fn | `set_rt_throttling` | `src/task/rt_scheduler.rs:423` |
| fn | `init` | `src/task/rt_scheduler.rs:448` |
| fn | `has_rt_tasks` | `src/task/rt_scheduler.rs:453` |
| fn | `rt_task_count` | `src/task/rt_scheduler.rs:458` |
| fn | `enqueue_rt_task` | `src/task/rt_scheduler.rs:463` |
| fn | `dequeue_rt_task` | `src/task/rt_scheduler.rs:468` |
| fn | `pick_next_rt_task` | `src/task/rt_scheduler.rs:474` |
| fn | `set_sched_param` | `src/task/rt_scheduler.rs:479` |
| fn | `get_sched_param` | `src/task/rt_scheduler.rs:484` |
| fn | `rt_tick` | `src/task/rt_scheduler.rs:490` |
| fn | `reenqueue_rr_task` | `src/task/rt_scheduler.rs:495` |
| fn | `set_rt_bandwidth` | `src/task/rt_scheduler.rs:500` |
| fn | `set_rt_throttling` | `src/task/rt_scheduler.rs:505` |
| fn | `is_rt_task` | `src/task/rt_scheduler.rs:510` |
| fn | `get_task_priority` | `src/task/rt_scheduler.rs:520` |
| fn | `get_task_policy` | `src/task/rt_scheduler.rs:530` |
| fn | `yield_rt_task` | `src/task/rt_scheduler.rs:542` |
| fn | `sys_sched_setscheduler` | `src/task/rt_scheduler.rs:559` |
| fn | `sys_sched_getscheduler` | `src/task/rt_scheduler.rs:581` |
| fn | `sys_sched_setparam` | `src/task/rt_scheduler.rs:586` |
| fn | `sys_sched_getparam` | `src/task/rt_scheduler.rs:593` |
| fn | `sys_sched_yield` | `src/task/rt_scheduler.rs:598` |
| fn | `sys_sched_get_priority_max` | `src/task/rt_scheduler.rs:604` |
| fn | `sys_sched_get_priority_min` | `src/task/rt_scheduler.rs:612` |
| fn | `sys_sched_rr_get_interval` | `src/task/rt_scheduler.rs:621` |

## A05 - CFS: vruntime adalet motoru

- Dosya: `src/task/cfs.rs`
- Public sembol sayisi: 36

| Tip | Sembol | Konum |
|---|---|---|
| const | `CFS_DEFAULT_SLICE` | `src/task/cfs.rs:49` |
| const | `CFS_MIN_GRANULARITY` | `src/task/cfs.rs:51` |
| const | `CFS_WAKEUP_GRANULARITY` | `src/task/cfs.rs:53` |
| const | `CFS_NICE_0_WEIGHT` | `src/task/cfs.rs:55` |
| const | `CFS_LOAD_AVG_PERIOD` | `src/task/cfs.rs:57` |
| const | `CFS_PELT_HALF_LIFE` | `src/task/cfs.rs:59` |
| fn | `nice_to_weight` | `src/task/cfs.rs:64` |
| fn | `weight_to_vruntime` | `src/task/cfs.rs:82` |
| struct | `CfsTask` | `src/task/cfs.rs:94` |
| struct | `CfsStats` | `src/task/cfs.rs:122` |
| fn | `new` | `src/task/cfs.rs:134` |
| fn | `set_nice` | `src/task/cfs.rs:152` |
| fn | `update_vruntime` | `src/task/cfs.rs:159` |
| fn | `calc_slice` | `src/task/cfs.rs:168` |
| fn | `is_eligible` | `src/task/cfs.rs:181` |
| struct | `CfsRq` | `src/task/cfs.rs:205` |
| fn | `new` | `src/task/cfs.rs:225` |
| fn | `enqueue` | `src/task/cfs.rs:241` |
| fn | `dequeue` | `src/task/cfs.rs:258` |
| fn | `pick_next` | `src/task/cfs.rs:269` |
| fn | `put_prev` | `src/task/cfs.rs:287` |
| fn | `update_clock` | `src/task/cfs.rs:298` |
| fn | `update_load_avg` | `src/task/cfs.rs:304` |
| fn | `check_preempt_wakeup` | `src/task/cfs.rs:315` |
| struct | `CfsScheduler` | `src/task/cfs.rs:334` |
| fn | `new` | `src/task/cfs.rs:348` |
| fn | `schedule` | `src/task/cfs.rs:364` |
| fn | `tick` | `src/task/cfs.rs:375` |
| fn | `enqueue` | `src/task/cfs.rs:401` |
| fn | `dequeue` | `src/task/cfs.rs:409` |
| fn | `load_balance` | `src/task/cfs.rs:418` |
| fn | `set_nice` | `src/task/cfs.rs:447` |
| fn | `sys_sched_setparam` | `src/task/cfs.rs:460` |
| fn | `sys_sched_getparam` | `src/task/cfs.rs:466` |
| fn | `sys_sched_yield` | `src/task/cfs.rs:470` |
| fn | `init` | `src/task/cfs.rs:479` |

## A06 - EEVDF: eligible_vtime ve virtual deadline

- Dosya: `src/task/eevdf.rs`
- Public sembol sayisi: 14

| Tip | Sembol | Konum |
|---|---|---|
| struct | `EevdfTask` | `src/task/eevdf.rs:17` |
| fn | `new` | `src/task/eevdf.rs:29` |
| fn | `update_runtime` | `src/task/eevdf.rs:44` |
| struct | `EevdfStats` | `src/task/eevdf.rs:64` |
| struct | `EevdfRunQueue` | `src/task/eevdf.rs:70` |
| fn | `new` | `src/task/eevdf.rs:77` |
| fn | `vtime` | `src/task/eevdf.rs:85` |
| fn | `enqueue` | `src/task/eevdf.rs:89` |
| fn | `dequeue` | `src/task/eevdf.rs:101` |
| fn | `account_runtime` | `src/task/eevdf.rs:112` |
| fn | `pick_next` | `src/task/eevdf.rs:135` |
| fn | `should_preempt` | `src/task/eevdf.rs:145` |
| fn | `stats` | `src/task/eevdf.rs:162` |
| fn | `ordered_task_ids` | `src/task/eevdf.rs:176` |

## A07 - Deadline scheduler: EDF/CBS admission ve replenish

- Dosya: `src/task/deadline.rs`
- Public sembol sayisi: 35

| Tip | Sembol | Konum |
|---|---|---|
| const | `DL_DEFAULT_RUNTIME` | `src/task/deadline.rs:59` |
| const | `DL_DEFAULT_PERIOD` | `src/task/deadline.rs:61` |
| const | `DL_DEFAULT_DEADLINE` | `src/task/deadline.rs:63` |
| const | `SCHED_DEADLINE` | `src/task/deadline.rs:66` |
| const | `SCHED_FLAG_DL_OVERRUN` | `src/task/deadline.rs:69` |
| const | `SCHED_FLAG_DL_RECLAIM` | `src/task/deadline.rs:70` |
| const | `SCHED_FLAG_DL_SPECIAL` | `src/task/deadline.rs:71` |
| struct | `DeadlineTask` | `src/task/deadline.rs:83` |
| struct | `DlStats` | `src/task/deadline.rs:109` |
| fn | `new` | `src/task/deadline.rs:117` |
| fn | `deadline_passed` | `src/task/deadline.rs:137` |
| fn | `runtime_exhausted` | `src/task/deadline.rs:143` |
| fn | `consume_runtime` | `src/task/deadline.rs:149` |
| fn | `replenish` | `src/task/deadline.rs:161` |
| fn | `laxity` | `src/task/deadline.rs:184` |
| fn | `compare_deadline` | `src/task/deadline.rs:193` |
| struct | `DeadlineRq` | `src/task/deadline.rs:213` |
| fn | `new` | `src/task/deadline.rs:225` |
| fn | `enqueue` | `src/task/deadline.rs:236` |
| fn | `dequeue` | `src/task/deadline.rs:254` |
| fn | `pick_next` | `src/task/deadline.rs:264` |
| fn | `check_replenishments` | `src/task/deadline.rs:293` |
| fn | `check_deadline_misses` | `src/task/deadline.rs:304` |
| struct | `DeadlineScheduler` | `src/task/deadline.rs:323` |
| fn | `new` | `src/task/deadline.rs:335` |
| fn | `schedule` | `src/task/deadline.rs:351` |
| fn | `add_task` | `src/task/deadline.rs:362` |
| fn | `remove_task` | `src/task/deadline.rs:372` |
| fn | `tick` | `src/task/deadline.rs:381` |
| fn | `set_bandwidth_cap` | `src/task/deadline.rs:400` |
| enum | `DlError` | `src/task/deadline.rs:417` |
| fn | `sys_sched_setattr` | `src/task/deadline.rs:428` |
| fn | `sys_sched_getattr` | `src/task/deadline.rs:438` |
| struct | `SchedAttr` | `src/task/deadline.rs:448` |
| fn | `init` | `src/task/deadline.rs:462` |

## A08 - Chase-Lev deque: lock-free race analizi

- Dosya: `src/task/deque.rs`
- Public sembol sayisi: 7

| Tip | Sembol | Konum |
|---|---|---|
| struct | `Worker` | `src/task/deque.rs:46` |
| struct | `Stealer` | `src/task/deque.rs:52` |
| fn | `new` | `src/task/deque.rs:70` |
| fn | `push` | `src/task/deque.rs:93` |
| fn | `pop` | `src/task/deque.rs:118` |
| fn | `len` | `src/task/deque.rs:163` |
| fn | `steal` | `src/task/deque.rs:179` |

## A09 - Hiyerarsik timing wheel

- Dosya: `src/task/timer.rs`
- Public sembol sayisi: 4

| Tip | Sembol | Konum |
|---|---|---|
| struct | `TimingWheel` | `src/task/timer.rs:53` |
| fn | `new` | `src/task/timer.rs:62` |
| fn | `schedule` | `src/task/timer.rs:88` |
| fn | `tick` | `src/task/timer.rs:122` |

## A10 - Zone-aware PMM fallback mimarisi

- Dosya: `src/memory/fibonacci_pmm.rs`
- Public sembol sayisi: 13

| Tip | Sembol | Konum |
|---|---|---|
| enum | `MemoryZone` | `src/memory/fibonacci_pmm.rs:71` |
| struct | `FibonacciPmm` | `src/memory/fibonacci_pmm.rs:122` |
| fn | `empty` | `src/memory/fibonacci_pmm.rs:138` |
| fn | `allocate_from_zone` | `src/memory/fibonacci_pmm.rs:223` |
| fn | `allocate_contiguous_from_zone` | `src/memory/fibonacci_pmm.rs:240` |
| fn | `allocate_frame` | `src/memory/fibonacci_pmm.rs:303` |
| fn | `allocate_contiguous` | `src/memory/fibonacci_pmm.rs:308` |
| fn | `deallocate_contiguous` | `src/memory/fibonacci_pmm.rs:313` |
| fn | `utilization` | `src/memory/fibonacci_pmm.rs:337` |
| fn | `total_frames` | `src/memory/fibonacci_pmm.rs:344` |
| fn | `free_frames` | `src/memory/fibonacci_pmm.rs:348` |
| fn | `zone_stats` | `src/memory/fibonacci_pmm.rs:353` |
| fn | `fragmentation` | `src/memory/fibonacci_pmm.rs:360` |

## A11 - Fibonacci buddy split/coalesce

- Dosya: `src/memory/fibonacci_buddy.rs`
- Public sembol sayisi: 6

| Tip | Sembol | Konum |
|---|---|---|
| struct | `FibonacciBuddyAllocator` | `src/memory/fibonacci_buddy.rs:68` |
| fn | `new` | `src/memory/fibonacci_buddy.rs:83` |
| fn | `allocate` | `src/memory/fibonacci_buddy.rs:132` |
| fn | `deallocate` | `src/memory/fibonacci_buddy.rs:159` |
| fn | `utilization` | `src/memory/fibonacci_buddy.rs:240` |
| fn | `fragmentation` | `src/memory/fibonacci_buddy.rs:250` |

## A12 - TLSF heap wrapper guvenligi

- Dosya: `src/allocator/tlsf.rs`
- Public sembol sayisi: 13

| Tip | Sembol | Konum |
|---|---|---|
| struct | `LockedTlsf` | `src/allocator/tlsf.rs:143` |
| const | `fn` | `src/allocator/tlsf.rs:154` |
| fn | `check_integrity` | `src/allocator/tlsf.rs:220` |
| fn | `corruption_count` | `src/allocator/tlsf.rs:251` |
| fn | `check_heap_integrity` | `src/allocator/tlsf.rs:256` |
| fn | `get_stats` | `src/allocator/tlsf.rs:265` |
| fn | `memory_stats` | `src/allocator/tlsf.rs:281` |
| struct | `IntegrityReport` | `src/allocator/tlsf.rs:321` |
| struct | `MemoryStats` | `src/allocator/tlsf.rs:330` |
| struct | `AllocStats` | `src/allocator/tlsf.rs:339` |
| fn | `heap_stats` | `src/allocator/tlsf.rs:533` |
| fn | `early_heap_usage` | `src/allocator/tlsf.rs:543` |
| fn | `main_heap_bounds` | `src/allocator/tlsf.rs:550` |

## A13 - User page fault, COW ve THP karari

- Dosya: `src/memory/mod.rs`
- Public sembol sayisi: 114

| Tip | Sembol | Konum |
|---|---|---|
| struct | `MemoryStats` | `src/memory/mod.rs:183` |
| const | `KERNEL_HEAP_BASE` | `src/memory/mod.rs:199` |
| const | `KERNEL_HEAP_SIZE` | `src/memory/mod.rs:201` |
| fn | `get_memory_stats` | `src/memory/mod.rs:206` |
| struct | `MemoryManager` | `src/memory/mod.rs:254` |
| fn | `new` | `src/memory/mod.rs:266` |
| fn | `get_memory_map` | `src/memory/mod.rs:277` |
| fn | `memory_map_mut` | `src/memory/mod.rs:281` |
| fn | `allocate_contiguous_frames` | `src/memory/mod.rs:285` |
| fn | `deallocate_contiguous_frames` | `src/memory/mod.rs:289` |
| fn | `total_frames` | `src/memory/mod.rs:300` |
| fn | `free_frames` | `src/memory/mod.rs:304` |
| fn | `init_uefi` | `src/memory/mod.rs:410` |
| fn | `init_memory_subsystems` | `src/memory/mod.rs:417` |
| fn | `global_memory_manager` | `src/memory/mod.rs:471` |
| const | `PAGE_SIZE` | `src/memory/mod.rs:498` |
| const | `PHYSICAL_MEMORY_OFFSET` | `src/memory/mod.rs:499` |
| const | `KERNEL_SPACE_START` | `src/memory/mod.rs:500` |
| const | `KERNEL_STACK_VIRT_BASE` | `src/memory/mod.rs:501` |
| const | `KERNEL_STACK_VIRT_LIMIT` | `src/memory/mod.rs:502` |
| const | `USER_SPACE_START` | `src/memory/mod.rs:503` |
| const | `USER_SPACE_END` | `src/memory/mod.rs:504` |
| const | `USER_STACK_TOP` | `src/memory/mod.rs:505` |
| const | `USER_STACK_PAGES` | `src/memory/mod.rs:506` |
| const | `USER_HEAP_BASE` | `src/memory/mod.rs:507` |
| const | `USER_MMAP_BASE` | `src/memory/mod.rs:508` |
| const | `USER_MMAP_RANDOM_RANGE` | `src/memory/mod.rs:509` |
| const | `USER_STACK_RANDOM_RANGE` | `src/memory/mod.rs:510` |
| const | `USER_HEAP_RANDOM_RANGE` | `src/memory/mod.rs:511` |
| struct | `AddressSpace` | `src/memory/mod.rs:1578` |
| fn | `active_physical_offset` | `src/memory/mod.rs:1754` |
| fn | `set_active_physical_offset` | `src/memory/mod.rs:1758` |
| fn | `set_kaslr_offset` | `src/memory/mod.rs:1764` |
| fn | `kaslr_offset` | `src/memory/mod.rs:1768` |
| fn | `kernel_virtual_base` | `src/memory/mod.rs:1772` |
| fn | `is_user_address` | `src/memory/mod.rs:1776` |
| fn | `is_user_range` | `src/memory/mod.rs:1780` |
| fn | `is_kernel_address` | `src/memory/mod.rs:1788` |
| fn | `create_address_space` | `src/memory/mod.rs:1792` |
| fn | `create_address_space_owned` | `src/memory/mod.rs:1806` |
| fn | `create_empty_address_space` | `src/memory/mod.rs:1820` |
| fn | `address_space_id` | `src/memory/mod.rs:1824` |
| fn | `allocate_user_mmap_in` | `src/memory/mod.rs:1828` |
| fn | `register_shared_anon_region_in` | `src/memory/mod.rs:1850` |
| fn | `clone_address_space_for_cow` | `src/memory/mod.rs:1888` |
| fn | `clone_user_pml4_for_cow` | `src/memory/mod.rs:1905` |
| fn | `set_active_address_space` | `src/memory/mod.rs:1967` |
| fn | `apply_cow_write_protect_current` | `src/memory/mod.rs:1971` |
| fn | `register_lazy_region` | `src/memory/mod.rs:2013` |
| fn | `register_shared_anon_region` | `src/memory/mod.rs:2045` |
| fn | `user_stack_bounds` | `src/memory/mod.rs:2118` |
| fn | `user_heap_limit` | `src/memory/mod.rs:2127` |
| fn | `allocate_user_mmap` | `src/memory/mod.rs:2134` |
| fn | `update_user_region_flags` | `src/memory/mod.rs:2157` |
| fn | `user_heap_state` | `src/memory/mod.rs:2230` |
| fn | `set_user_heap_break` | `src/memory/mod.rs:2242` |
| fn | `init_swap_device` | `src/memory/mod.rs:2422` |
| fn | `start_reclaim_daemon` | `src/memory/mod.rs:2438` |
| fn | `reclaim_pages` | `src/memory/mod.rs:2646` |
| fn | `reclaim_pages_global` | `src/memory/mod.rs:2650` |
| fn | `unmap_user_range` | `src/memory/mod.rs:2654` |
| fn | `register_cow_region` | `src/memory/mod.rs:2775` |
| fn | `set_user_image` | `src/memory/mod.rs:2829` |
| fn | `set_user_image_owned` | `src/memory/mod.rs:2835` |
| fn | `register_file_lazy_region` | `src/memory/mod.rs:2841` |
| fn | `register_file_backed_region` | `src/memory/mod.rs:2881` |
| fn | `user_region_overlaps` | `src/memory/mod.rs:2926` |
| fn | `user_stack_guards_region` | `src/memory/mod.rs:2950` |
| fn | `user_heap_guards_region` | `src/memory/mod.rs:2971` |
| fn | `handle_user_page_fault` | `src/memory/mod.rs:2992` |
| fn | `audit_user_mappings` | `src/memory/mod.rs:3086` |
| fn | `audit_kernel_user_flags` | `src/memory/mod.rs:3111` |
| fn | `audit_page_table_security` | `src/memory/mod.rs:3123` |
| fn | `allocate_contiguous_frames` | `src/memory/mod.rs:3613` |
| fn | `alloc_phys` | `src/memory/mod.rs:3621` |
| fn | `free_phys` | `src/memory/mod.rs:3629` |
| fn | `deallocate_contiguous_frames` | `src/memory/mod.rs:3638` |
| struct | `DeviceId` | `src/memory/mod.rs:3655` |
| fn | `dma_alloc` | `src/memory/mod.rs:3740` |
| fn | `dma_dealloc` | `src/memory/mod.rs:3768` |
| fn | `dma_alloc` | `src/memory/mod.rs:3784` |
| fn | `dma_dealloc` | `src/memory/mod.rs:3806` |
| fn | `dma_share` | `src/memory/mod.rs:3823` |
| fn | `dma_unshare` | `src/memory/mod.rs:3866` |
| fn | `init_iommu` | `src/memory/mod.rs:3879` |
| fn | `iommu_enabled` | `src/memory/mod.rs:3922` |
| fn | `iommu_register_device` | `src/memory/mod.rs:3926` |
| fn | `iommu_domain_for_device` | `src/memory/mod.rs:3961` |
| fn | `dma_alloc_for_domain` | `src/memory/mod.rs:4017` |
| fn | `dma_dealloc_for_domain` | `src/memory/mod.rs:4027` |
| fn | `dma_share_for_domain` | `src/memory/mod.rs:4036` |
| fn | `dma_unshare_for_domain` | `src/memory/mod.rs:4050` |
| fn | `phys_to_virt` | `src/memory/mod.rs:4060` |
| fn | `virt_to_phys` | `src/memory/mod.rs:4064` |
| fn | `virt_to_phys_u64` | `src/memory/mod.rs:4075` |
| fn | `virt_to_phys_va` | `src/memory/mod.rs:4087` |
| fn | `hhdm_offset` | `src/memory/mod.rs:4106` |
| fn | `alloc_zeroed_page` | `src/memory/mod.rs:4121` |
| fn | `map_physical_to_user_va` | `src/memory/mod.rs:4146` |
| fn | `unmap_user_va` | `src/memory/mod.rs:4189` |
| fn | `translate_addr` | `src/memory/mod.rs:4218` |
| fn | `is_kernel_stack_virt_addr` | `src/memory/mod.rs:4223` |
| fn | `map_kernel_stack_pages` | `src/memory/mod.rs:4227` |
| fn | `unmap_kernel_stack_pages` | `src/memory/mod.rs:4278` |
| fn | `unmap_kernel_guard_page` | `src/memory/mod.rs:4310` |
| fn | `remap_kernel_guard_page` | `src/memory/mod.rs:4355` |
| fn | `create_user_pml4` | `src/memory/mod.rs:4460` |
| fn | `map_mmio` | `src/memory/mod.rs:4492` |
| fn | `map_identity` | `src/memory/mod.rs:4597` |
| fn | `ensure_identity_mapped` | `src/memory/mod.rs:4750` |
| enum | `VmmInitError` | `src/memory/mod.rs:4807` |
| enum | `UefiHhdmError` | `src/memory/mod.rs:5039` |
| fn | `init_uefi_hhdm` | `src/memory/mod.rs:5062` |
| fn | `set_uefi_virtual_address_map` | `src/memory/mod.rs:5248` |

## A14 - Reclaim daemon, writeback budget ve pressure

- Dosya: `src/memory/mod.rs`
- Public sembol sayisi: 114

| Tip | Sembol | Konum |
|---|---|---|
| struct | `MemoryStats` | `src/memory/mod.rs:183` |
| const | `KERNEL_HEAP_BASE` | `src/memory/mod.rs:199` |
| const | `KERNEL_HEAP_SIZE` | `src/memory/mod.rs:201` |
| fn | `get_memory_stats` | `src/memory/mod.rs:206` |
| struct | `MemoryManager` | `src/memory/mod.rs:254` |
| fn | `new` | `src/memory/mod.rs:266` |
| fn | `get_memory_map` | `src/memory/mod.rs:277` |
| fn | `memory_map_mut` | `src/memory/mod.rs:281` |
| fn | `allocate_contiguous_frames` | `src/memory/mod.rs:285` |
| fn | `deallocate_contiguous_frames` | `src/memory/mod.rs:289` |
| fn | `total_frames` | `src/memory/mod.rs:300` |
| fn | `free_frames` | `src/memory/mod.rs:304` |
| fn | `init_uefi` | `src/memory/mod.rs:410` |
| fn | `init_memory_subsystems` | `src/memory/mod.rs:417` |
| fn | `global_memory_manager` | `src/memory/mod.rs:471` |
| const | `PAGE_SIZE` | `src/memory/mod.rs:498` |
| const | `PHYSICAL_MEMORY_OFFSET` | `src/memory/mod.rs:499` |
| const | `KERNEL_SPACE_START` | `src/memory/mod.rs:500` |
| const | `KERNEL_STACK_VIRT_BASE` | `src/memory/mod.rs:501` |
| const | `KERNEL_STACK_VIRT_LIMIT` | `src/memory/mod.rs:502` |
| const | `USER_SPACE_START` | `src/memory/mod.rs:503` |
| const | `USER_SPACE_END` | `src/memory/mod.rs:504` |
| const | `USER_STACK_TOP` | `src/memory/mod.rs:505` |
| const | `USER_STACK_PAGES` | `src/memory/mod.rs:506` |
| const | `USER_HEAP_BASE` | `src/memory/mod.rs:507` |
| const | `USER_MMAP_BASE` | `src/memory/mod.rs:508` |
| const | `USER_MMAP_RANDOM_RANGE` | `src/memory/mod.rs:509` |
| const | `USER_STACK_RANDOM_RANGE` | `src/memory/mod.rs:510` |
| const | `USER_HEAP_RANDOM_RANGE` | `src/memory/mod.rs:511` |
| struct | `AddressSpace` | `src/memory/mod.rs:1578` |
| fn | `active_physical_offset` | `src/memory/mod.rs:1754` |
| fn | `set_active_physical_offset` | `src/memory/mod.rs:1758` |
| fn | `set_kaslr_offset` | `src/memory/mod.rs:1764` |
| fn | `kaslr_offset` | `src/memory/mod.rs:1768` |
| fn | `kernel_virtual_base` | `src/memory/mod.rs:1772` |
| fn | `is_user_address` | `src/memory/mod.rs:1776` |
| fn | `is_user_range` | `src/memory/mod.rs:1780` |
| fn | `is_kernel_address` | `src/memory/mod.rs:1788` |
| fn | `create_address_space` | `src/memory/mod.rs:1792` |
| fn | `create_address_space_owned` | `src/memory/mod.rs:1806` |
| fn | `create_empty_address_space` | `src/memory/mod.rs:1820` |
| fn | `address_space_id` | `src/memory/mod.rs:1824` |
| fn | `allocate_user_mmap_in` | `src/memory/mod.rs:1828` |
| fn | `register_shared_anon_region_in` | `src/memory/mod.rs:1850` |
| fn | `clone_address_space_for_cow` | `src/memory/mod.rs:1888` |
| fn | `clone_user_pml4_for_cow` | `src/memory/mod.rs:1905` |
| fn | `set_active_address_space` | `src/memory/mod.rs:1967` |
| fn | `apply_cow_write_protect_current` | `src/memory/mod.rs:1971` |
| fn | `register_lazy_region` | `src/memory/mod.rs:2013` |
| fn | `register_shared_anon_region` | `src/memory/mod.rs:2045` |
| fn | `user_stack_bounds` | `src/memory/mod.rs:2118` |
| fn | `user_heap_limit` | `src/memory/mod.rs:2127` |
| fn | `allocate_user_mmap` | `src/memory/mod.rs:2134` |
| fn | `update_user_region_flags` | `src/memory/mod.rs:2157` |
| fn | `user_heap_state` | `src/memory/mod.rs:2230` |
| fn | `set_user_heap_break` | `src/memory/mod.rs:2242` |
| fn | `init_swap_device` | `src/memory/mod.rs:2422` |
| fn | `start_reclaim_daemon` | `src/memory/mod.rs:2438` |
| fn | `reclaim_pages` | `src/memory/mod.rs:2646` |
| fn | `reclaim_pages_global` | `src/memory/mod.rs:2650` |
| fn | `unmap_user_range` | `src/memory/mod.rs:2654` |
| fn | `register_cow_region` | `src/memory/mod.rs:2775` |
| fn | `set_user_image` | `src/memory/mod.rs:2829` |
| fn | `set_user_image_owned` | `src/memory/mod.rs:2835` |
| fn | `register_file_lazy_region` | `src/memory/mod.rs:2841` |
| fn | `register_file_backed_region` | `src/memory/mod.rs:2881` |
| fn | `user_region_overlaps` | `src/memory/mod.rs:2926` |
| fn | `user_stack_guards_region` | `src/memory/mod.rs:2950` |
| fn | `user_heap_guards_region` | `src/memory/mod.rs:2971` |
| fn | `handle_user_page_fault` | `src/memory/mod.rs:2992` |
| fn | `audit_user_mappings` | `src/memory/mod.rs:3086` |
| fn | `audit_kernel_user_flags` | `src/memory/mod.rs:3111` |
| fn | `audit_page_table_security` | `src/memory/mod.rs:3123` |
| fn | `allocate_contiguous_frames` | `src/memory/mod.rs:3613` |
| fn | `alloc_phys` | `src/memory/mod.rs:3621` |
| fn | `free_phys` | `src/memory/mod.rs:3629` |
| fn | `deallocate_contiguous_frames` | `src/memory/mod.rs:3638` |
| struct | `DeviceId` | `src/memory/mod.rs:3655` |
| fn | `dma_alloc` | `src/memory/mod.rs:3740` |
| fn | `dma_dealloc` | `src/memory/mod.rs:3768` |
| fn | `dma_alloc` | `src/memory/mod.rs:3784` |
| fn | `dma_dealloc` | `src/memory/mod.rs:3806` |
| fn | `dma_share` | `src/memory/mod.rs:3823` |
| fn | `dma_unshare` | `src/memory/mod.rs:3866` |
| fn | `init_iommu` | `src/memory/mod.rs:3879` |
| fn | `iommu_enabled` | `src/memory/mod.rs:3922` |
| fn | `iommu_register_device` | `src/memory/mod.rs:3926` |
| fn | `iommu_domain_for_device` | `src/memory/mod.rs:3961` |
| fn | `dma_alloc_for_domain` | `src/memory/mod.rs:4017` |
| fn | `dma_dealloc_for_domain` | `src/memory/mod.rs:4027` |
| fn | `dma_share_for_domain` | `src/memory/mod.rs:4036` |
| fn | `dma_unshare_for_domain` | `src/memory/mod.rs:4050` |
| fn | `phys_to_virt` | `src/memory/mod.rs:4060` |
| fn | `virt_to_phys` | `src/memory/mod.rs:4064` |
| fn | `virt_to_phys_u64` | `src/memory/mod.rs:4075` |
| fn | `virt_to_phys_va` | `src/memory/mod.rs:4087` |
| fn | `hhdm_offset` | `src/memory/mod.rs:4106` |
| fn | `alloc_zeroed_page` | `src/memory/mod.rs:4121` |
| fn | `map_physical_to_user_va` | `src/memory/mod.rs:4146` |
| fn | `unmap_user_va` | `src/memory/mod.rs:4189` |
| fn | `translate_addr` | `src/memory/mod.rs:4218` |
| fn | `is_kernel_stack_virt_addr` | `src/memory/mod.rs:4223` |
| fn | `map_kernel_stack_pages` | `src/memory/mod.rs:4227` |
| fn | `unmap_kernel_stack_pages` | `src/memory/mod.rs:4278` |
| fn | `unmap_kernel_guard_page` | `src/memory/mod.rs:4310` |
| fn | `remap_kernel_guard_page` | `src/memory/mod.rs:4355` |
| fn | `create_user_pml4` | `src/memory/mod.rs:4460` |
| fn | `map_mmio` | `src/memory/mod.rs:4492` |
| fn | `map_identity` | `src/memory/mod.rs:4597` |
| fn | `ensure_identity_mapped` | `src/memory/mod.rs:4750` |
| enum | `VmmInitError` | `src/memory/mod.rs:4807` |
| enum | `UefiHhdmError` | `src/memory/mod.rs:5039` |
| fn | `init_uefi_hhdm` | `src/memory/mod.rs:5062` |
| fn | `set_uefi_virtual_address_map` | `src/memory/mod.rs:5248` |

## A15 - MGLRU generation ve victim secimi

- Dosya: `src/memory/mglru.rs`
- Public sembol sayisi: 12

| Tip | Sembol | Konum |
|---|---|---|
| struct | `MglruPageKey` | `src/memory/mglru.rs:19` |
| struct | `MglruVictim` | `src/memory/mglru.rs:25` |
| struct | `MglruStats` | `src/memory/mglru.rs:42` |
| fn | `init` | `src/memory/mglru.rs:256` |
| fn | `is_enabled` | `src/memory/mglru.rs:260` |
| fn | `record_page_access` | `src/memory/mglru.rs:264` |
| fn | `age_generations` | `src/memory/mglru.rs:285` |
| fn | `record_refault` | `src/memory/mglru.rs:297` |
| fn | `record_eviction` | `src/memory/mglru.rs:310` |
| fn | `remove_page` | `src/memory/mglru.rs:320` |
| fn | `pick_victim` | `src/memory/mglru.rs:330` |
| fn | `get_stats` | `src/memory/mglru.rs:337` |

## A16 - ZSwap compression pipeline

- Dosya: `src/memory/zswap.rs`
- Public sembol sayisi: 37

| Tip | Sembol | Konum |
|---|---|---|
| const | `ZSWAP_MAX_POOL_PERCENT` | `src/memory/zswap.rs:81` |
| const | `ZSWAP_DEFAULT_POOL_PERCENT` | `src/memory/zswap.rs:83` |
| const | `ZSWAP_MAX_ZBUD_PAGES` | `src/memory/zswap.rs:85` |
| const | `ZSWAP_DEFAULT_COMPRESSOR` | `src/memory/zswap.rs:87` |
| struct | `Lz4Compressor` | `src/memory/zswap.rs:105` |
| struct | `ZstdCompressor` | `src/memory/zswap.rs:230` |
| struct | `ZswapEntry` | `src/memory/zswap.rs:254` |
| fn | `new` | `src/memory/zswap.rs:270` |
| fn | `compression_ratio` | `src/memory/zswap.rs:288` |
| struct | `ZswapPool` | `src/memory/zswap.rs:314` |
| fn | `new` | `src/memory/zswap.rs:332` |
| fn | `store` | `src/memory/zswap.rs:345` |
| fn | `load` | `src/memory/zswap.rs:377` |
| fn | `remove` | `src/memory/zswap.rs:391` |
| fn | `compression_ratio` | `src/memory/zswap.rs:425` |
| struct | `ZswapStats` | `src/memory/zswap.rs:441` |
| struct | `ZswapManager` | `src/memory/zswap.rs:458` |
| fn | `new` | `src/memory/zswap.rs:474` |
| fn | `init` | `src/memory/zswap.rs:486` |
| fn | `store` | `src/memory/zswap.rs:505` |
| fn | `load` | `src/memory/zswap.rs:540` |
| fn | `invalidate` | `src/memory/zswap.rs:552` |
| fn | `writeback_lru` | `src/memory/zswap.rs:561` |
| fn | `compression_ratio` | `src/memory/zswap.rs:569` |
| fn | `get_stats` | `src/memory/zswap.rs:578` |
| fn | `set_max_pool_percent` | `src/memory/zswap.rs:583` |
| fn | `set_enabled` | `src/memory/zswap.rs:589` |
| struct | `ZramDevice` | `src/memory/zswap.rs:604` |
| struct | `ZramStats` | `src/memory/zswap.rs:618` |
| fn | `new` | `src/memory/zswap.rs:638` |
| fn | `write` | `src/memory/zswap.rs:649` |
| fn | `read` | `src/memory/zswap.rs:675` |
| fn | `set_size` | `src/memory/zswap.rs:688` |
| fn | `reset` | `src/memory/zswap.rs:694` |
| enum | `ZswapError` | `src/memory/zswap.rs:706` |
| fn | `init` | `src/memory/zswap.rs:722` |
| fn | `is_enabled` | `src/memory/zswap.rs:728` |

## A17 - Lock-free io_uring publication boundaries

- Dosya: `src/posix/io_uring_ring.rs`
- Public sembol sayisi: 40

| Tip | Sembol | Konum |
|---|---|---|
| struct | `SendPtr` | `src/posix/io_uring_ring.rs:47` |
| fn | `new` | `src/posix/io_uring_ring.rs:53` |
| fn | `as_ptr` | `src/posix/io_uring_ring.rs:57` |
| struct | `RingSqe` | `src/posix/io_uring_ring.rs:79` |
| struct | `RingCqe` | `src/posix/io_uring_ring.rs:134` |
| const | `IORING_OP_NOP` | `src/posix/io_uring_ring.rs:158` |
| const | `IORING_OP_READV` | `src/posix/io_uring_ring.rs:160` |
| const | `IORING_OP_WRITEV` | `src/posix/io_uring_ring.rs:162` |
| const | `IORING_OP_FSYNC` | `src/posix/io_uring_ring.rs:164` |
| const | `IORING_OP_READ_FIXED` | `src/posix/io_uring_ring.rs:166` |
| const | `IORING_OP_WRITE_FIXED` | `src/posix/io_uring_ring.rs:168` |
| const | `IORING_OP_POLL_ADD` | `src/posix/io_uring_ring.rs:170` |
| const | `IORING_OP_POLL_REMOVE` | `src/posix/io_uring_ring.rs:172` |
| const | `IORING_OP_READ` | `src/posix/io_uring_ring.rs:174` |
| const | `IORING_OP_WRITE` | `src/posix/io_uring_ring.rs:176` |
| struct | `SubmissionRing` | `src/posix/io_uring_ring.rs:195` |
| struct | `CompletionRing` | `src/posix/io_uring_ring.rs:215` |
| const | `fn` | `src/posix/io_uring_ring.rs:232` |
| fn | `pending_count` | `src/posix/io_uring_ring.rs:261` |
| fn | `is_full` | `src/posix/io_uring_ring.rs:269` |
| fn | `is_empty` | `src/posix/io_uring_ring.rs:275` |
| fn | `capacity` | `src/posix/io_uring_ring.rs:281` |
| fn | `push` | `src/posix/io_uring_ring.rs:298` |
| fn | `pop` | `src/posix/io_uring_ring.rs:338` |
| fn | `pop_batch` | `src/posix/io_uring_ring.rs:371` |
| const | `fn` | `src/posix/io_uring_ring.rs:403` |
| fn | `pending_count` | `src/posix/io_uring_ring.rs:420` |
| fn | `is_full` | `src/posix/io_uring_ring.rs:428` |
| fn | `is_empty` | `src/posix/io_uring_ring.rs:434` |
| fn | `push` | `src/posix/io_uring_ring.rs:447` |
| fn | `pop` | `src/posix/io_uring_ring.rs:489` |
| fn | `pop_batch` | `src/posix/io_uring_ring.rs:513` |
| fn | `drain_overflow` | `src/posix/io_uring_ring.rs:541` |
| struct | `LockFreeIoUring` | `src/posix/io_uring_ring.rs:560` |
| const | `fn` | `src/posix/io_uring_ring.rs:574` |
| fn | `process_submissions` | `src/posix/io_uring_ring.rs:594` |
| fn | `completions_available` | `src/posix/io_uring_ring.rs:751` |
| fn | `submissions_pending` | `src/posix/io_uring_ring.rs:757` |
| fn | `cq_overflow_count` | `src/posix/io_uring_ring.rs:762` |
| fn | `sq_dropped_count` | `src/posix/io_uring_ring.rs:767` |

## A18 - TLS 1.3 handshake ve key schedule

- Dosya: `src/net/tls.rs`
- Public sembol sayisi: 131

| Tip | Sembol | Konum |
|---|---|---|
| const | `TLS_VERSION_1_3` | `src/net/tls.rs:97` |
| enum | `ContentType` | `src/net/tls.rs:107` |
| fn | `from_u8` | `src/net/tls.rs:115` |
| enum | `HandshakeType` | `src/net/tls.rs:131` |
| fn | `from_u8` | `src/net/tls.rs:145` |
| enum | `CipherSuite` | `src/net/tls.rs:167` |
| fn | `from_u16` | `src/net/tls.rs:174` |
| fn | `key_len` | `src/net/tls.rs:183` |
| fn | `iv_len` | `src/net/tls.rs:191` |
| enum | `NamedGroup` | `src/net/tls.rs:198` |
| fn | `from_u16` | `src/net/tls.rs:207` |
| enum | `SignatureScheme` | `src/net/tls.rs:224` |
| enum | `TlsError` | `src/net/tls.rs:243` |
| enum | `AlertLevel` | `src/net/tls.rs:259` |
| enum | `AlertDescription` | `src/net/tls.rs:266` |
| enum | `TlsState` | `src/net/tls.rs:295` |
| struct | `TlsRecordHeader` | `src/net/tls.rs:324` |
| const | `SIZE` | `src/net/tls.rs:331` |
| fn | `parse` | `src/net/tls.rs:333` |
| fn | `to_bytes` | `src/net/tls.rs:349` |
| struct | `HandshakeHeader` | `src/net/tls.rs:374` |
| const | `SIZE` | `src/net/tls.rs:380` |
| fn | `parse` | `src/net/tls.rs:382` |
| fn | `to_bytes` | `src/net/tls.rs:393` |
| struct | `KeySchedule` | `src/net/tls.rs:420` |
| fn | `new` | `src/net/tls.rs:431` |
| fn | `derive_handshake_secret` | `src/net/tls.rs:443` |
| fn | `derive_master_secret` | `src/net/tls.rs:459` |
| fn | `client_handshake_traffic_secret` | `src/net/tls.rs:515` |
| fn | `server_handshake_traffic_secret` | `src/net/tls.rs:519` |
| fn | `client_application_traffic_secret` | `src/net/tls.rs:523` |
| fn | `server_application_traffic_secret` | `src/net/tls.rs:527` |
| fn | `server_finished_verify_data` | `src/net/tls.rs:531` |
| fn | `resumption_psk` | `src/net/tls.rs:539` |
| struct | `TlsClient` | `src/net/tls.rs:580` |
| fn | `new` | `src/net/tls.rs:600` |
| fn | `build_client_hello` | `src/net/tls.rs:634` |
| fn | `process_server_hello` | `src/net/tls.rs:794` |
| fn | `process_encrypted_extensions` | `src/net/tls.rs:936` |
| fn | `process_certificate` | `src/net/tls.rs:959` |
| fn | `process_certificate_verify` | `src/net/tls.rs:981` |
| fn | `process_finished` | `src/net/tls.rs:1020` |
| fn | `complete_handshake` | `src/net/tls.rs:1054` |
| fn | `process_new_session_ticket` | `src/net/tls.rs:1067` |
| fn | `state` | `src/net/tls.rs:1099` |
| fn | `is_established` | `src/net/tls.rs:1102` |
| fn | `cipher_suite` | `src/net/tls.rs:1105` |
| fn | `wrap_record` | `src/net/tls.rs:1377` |
| fn | `parse_record` | `src/net/tls.rs:1393` |
| fn | `transcript_hash` | `src/net/tls.rs:1404` |
| struct | `Aes` | `src/net/tls.rs:1471` |
| fn | `new` | `src/net/tls.rs:1478` |
| fn | `encrypt_block` | `src/net/tls.rs:1620` |
| fn | `decrypt_block` | `src/net/tls.rs:1660` |
| struct | `AesGcm` | `src/net/tls.rs:1812` |
| fn | `new` | `src/net/tls.rs:1818` |
| fn | `encrypt` | `src/net/tls.rs:1830` |
| fn | `decrypt` | `src/net/tls.rs:1875` |
| struct | `ChaCha20` | `src/net/tls.rs:2027` |
| fn | `new` | `src/net/tls.rs:2039` |
| fn | `block` | `src/net/tls.rs:2098` |
| fn | `process` | `src/net/tls.rs:2135` |
| struct | `Poly1305` | `src/net/tls.rs:2177` |
| fn | `new` | `src/net/tls.rs:2197` |
| fn | `update` | `src/net/tls.rs:2315` |
| fn | `finalize` | `src/net/tls.rs:2354` |
| struct | `ChaCha20Poly1305` | `src/net/tls.rs:2436` |
| fn | `new` | `src/net/tls.rs:2441` |
| fn | `encrypt` | `src/net/tls.rs:2453` |
| fn | `decrypt` | `src/net/tls.rs:2477` |
| struct | `FieldElement` | `src/net/tls.rs:2540` |
| fn | `zero` | `src/net/tls.rs:2553` |
| fn | `one` | `src/net/tls.rs:2558` |
| fn | `from_bytes` | `src/net/tls.rs:2563` |
| fn | `to_bytes` | `src/net/tls.rs:2612` |
| fn | `add` | `src/net/tls.rs:2673` |
| fn | `sub` | `src/net/tls.rs:2682` |
| fn | `mul` | `src/net/tls.rs:2693` |
| fn | `square` | `src/net/tls.rs:2722` |
| fn | `reduce` | `src/net/tls.rs:2727` |
| fn | `invert` | `src/net/tls.rs:2746` |
| fn | `conditional_swap` | `src/net/tls.rs:2768` |
| struct | `X25519` | `src/net/tls.rs:2786` |
| fn | `generate_keypair` | `src/net/tls.rs:2793` |
| fn | `public_from_private` | `src/net/tls.rs:2807` |
| fn | `scalar_mult` | `src/net/tls.rs:2814` |
| fn | `diffie_hellman` | `src/net/tls.rs:2895` |
| struct | `TlsKeySchedule` | `src/net/tls.rs:2926` |
| fn | `new` | `src/net/tls.rs:2949` |
| fn | `hkdf_expand_label` | `src/net/tls.rs:3087` |
| fn | `derive_secret` | `src/net/tls.rs:3113` |
| fn | `init_with_psk` | `src/net/tls.rs:3118` |
| fn | `derive_handshake_secrets` | `src/net/tls.rs:3127` |
| fn | `derive_master_secret` | `src/net/tls.rs:3144` |
| fn | `derive_traffic_keys` | `src/net/tls.rs:3166` |
| fn | `client_hs_secret` | `src/net/tls.rs:3183` |
| fn | `server_hs_secret` | `src/net/tls.rs:3188` |
| fn | `client_app_secret` | `src/net/tls.rs:3193` |
| fn | `server_app_secret` | `src/net/tls.rs:3198` |
| fn | `compute_finished_mac` | `src/net/tls.rs:3205` |
| fn | `update_traffic_secret` | `src/net/tls.rs:3211` |
| struct | `TlsCrypto` | `src/net/tls.rs:3217` |
| fn | `new` | `src/net/tls.rs:3224` |
| fn | `encrypt_record` | `src/net/tls.rs:3233` |
| fn | `decrypt_record` | `src/net/tls.rs:3267` |
| struct | `EarlyDataConfig` | `src/net/tls.rs:3334` |
| struct | `SessionTicket` | `src/net/tls.rs:3352` |
| fn | `new` | `src/net/tls.rs:3375` |
| fn | `is_valid` | `src/net/tls.rs:3394` |
| fn | `obfuscated_age` | `src/net/tls.rs:3401` |
| fn | `derive_early_secret` | `src/net/tls.rs:3413` |
| enum | `EarlyDataState` | `src/net/tls.rs:3449` |
| struct | `ZeroRttState` | `src/net/tls.rs:3462` |
| fn | `new` | `src/net/tls.rs:3477` |
| fn | `with_ticket` | `src/net/tls.rs:3488` |
| fn | `can_send_early_data` | `src/net/tls.rs:3500` |
| fn | `send_early_data` | `src/net/tls.rs:3509` |
| fn | `on_reject` | `src/net/tls.rs:3532` |
| fn | `on_accept` | `src/net/tls.rs:3539` |
| fn | `get_retry_data` | `src/net/tls.rs:3544` |
| struct | `SessionCache` | `src/net/tls.rs:3586` |
| const | `fn` | `src/net/tls.rs:3592` |
| fn | `add` | `src/net/tls.rs:3600` |
| fn | `find_for_server` | `src/net/tls.rs:3613` |
| fn | `remove` | `src/net/tls.rs:3623` |
| fn | `clear` | `src/net/tls.rs:3628` |
| struct | `TlsHandshakeExt` | `src/net/tls.rs:3673` |
| fn | `new` | `src/net/tls.rs:3691` |
| fn | `start_with_early_data` | `src/net/tls.rs:3704` |
| fn | `process_server_response` | `src/net/tls.rs:3852` |
| fn | `send_data` | `src/net/tls.rs:3925` |

## A19 - QUIC frame parser ve ACK guard

- Dosya: `src/net/quic.rs`
- Public sembol sayisi: 62

| Tip | Sembol | Konum |
|---|---|---|
| const | `QUIC_VERSION_1` | `src/net/quic.rs:103` |
| enum | `QuicPacketType` | `src/net/quic.rs:109` |
| enum | `QuicFrameType` | `src/net/quic.rs:125` |
| enum | `QuicError` | `src/net/quic.rs:175` |
| struct | `ConnectionId` | `src/net/quic.rs:225` |
| fn | `new` | `src/net/quic.rs:232` |
| fn | `random` | `src/net/quic.rs:238` |
| fn | `len` | `src/net/quic.rs:247` |
| fn | `is_empty` | `src/net/quic.rs:252` |
| fn | `as_slice` | `src/net/quic.rs:257` |
| enum | `StreamType` | `src/net/quic.rs:285` |
| enum | `StreamState` | `src/net/quic.rs:298` |
| struct | `QuicStream` | `src/net/quic.rs:317` |
| fn | `new` | `src/net/quic.rs:345` |
| fn | `can_read` | `src/net/quic.rs:362` |
| fn | `can_write` | `src/net/quic.rs:368` |
| fn | `write` | `src/net/quic.rs:377` |
| fn | `read` | `src/net/quic.rs:393` |
| enum | `QuicFrame` | `src/net/quic.rs:424` |
| fn | `encode` | `src/net/quic.rs:508` |
| fn | `decode` | `src/net/quic.rs:712` |
| enum | `QuicState` | `src/net/quic.rs:838` |
| enum | `QuicCryptoLevel` | `src/net/quic.rs:859` |
| struct | `QuicKeys` | `src/net/quic.rs:870` |
| struct | `QuicConnection` | `src/net/quic.rs:883` |
| fn | `new` | `src/net/quic.rs:934` |
| fn | `create_stream` | `src/net/quic.rs:963` |
| fn | `get_stream` | `src/net/quic.rs:976` |
| fn | `get_stream_mut` | `src/net/quic.rs:981` |
| fn | `on_packet` | `src/net/quic.rs:986` |
| fn | `build_packet` | `src/net/quic.rs:1092` |
| struct | `QuicClient` | `src/net/quic.rs:1166` |
| fn | `new` | `src/net/quic.rs:1172` |
| fn | `connect` | `src/net/quic.rs:1180` |
| fn | `send` | `src/net/quic.rs:1245` |
| fn | `create_stream` | `src/net/quic.rs:1267` |
| struct | `QuicServer` | `src/net/quic.rs:1277` |
| fn | `new` | `src/net/quic.rs:1282` |
| fn | `on_packet` | `src/net/quic.rs:1289` |
| fn | `compute_nonce` | `src/net/quic.rs:1431` |
| fn | `compute_header_protection_mask` | `src/net/quic.rs:1448` |
| fn | `protect_long_header` | `src/net/quic.rs:1602` |
| fn | `unprotect_long_header` | `src/net/quic.rs:1627` |
| fn | `encrypt_packet_payload` | `src/net/quic.rs:1650` |
| fn | `decrypt_packet_payload` | `src/net/quic.rs:1684` |
| struct | `SentPacket` | `src/net/quic.rs:1719` |
| struct | `LossRecovery` | `src/net/quic.rs:1730` |
| enum | `CongestionState` | `src/net/quic.rs:1779` |
| fn | `new` | `src/net/quic.rs:1787` |
| fn | `on_packet_sent` | `src/net/quic.rs:1813` |
| fn | `on_ack_received` | `src/net/quic.rs:1839` |
| fn | `update_rtt` | `src/net/quic.rs:1878` |
| fn | `detect_lost_packets` | `src/net/quic.rs:1910` |
| fn | `pto` | `src/net/quic.rs:1968` |
| fn | `loss_detection_timeout` | `src/net/quic.rs:1977` |
| fn | `on_pto_expired` | `src/net/quic.rs:2007` |
| fn | `can_send` | `src/net/quic.rs:2073` |
| fn | `send_window` | `src/net/quic.rs:2078` |
| fn | `derive_initial_secret` | `src/net/quic.rs:2094` |
| fn | `hmac_sha256` | `src/net/quic.rs:2165` |
| fn | `sha256_hash` | `src/net/quic.rs:2206` |
| fn | `update_key` | `src/net/quic.rs:2299` |

## A20 - WireGuard handshake, nonce ve replay koruma

- Dosya: `src/net/wireguard.rs`
- Public sembol sayisi: 36

| Tip | Sembol | Konum |
|---|---|---|
| const | `WG_DEFAULT_PORT` | `src/net/wireguard.rs:58` |
| const | `WG_KEY_SIZE` | `src/net/wireguard.rs:61` |
| const | `WG_MSG_INITIATION` | `src/net/wireguard.rs:64` |
| const | `WG_MSG_RESPONSE` | `src/net/wireguard.rs:66` |
| const | `WG_MSG_COOKIE_REPLY` | `src/net/wireguard.rs:68` |
| const | `WG_MSG_TRANSPORT` | `src/net/wireguard.rs:70` |
| struct | `WgKey` | `src/net/wireguard.rs:88` |
| fn | `new` | `src/net/wireguard.rs:92` |
| fn | `from_bytes` | `src/net/wireguard.rs:97` |
| fn | `generate` | `src/net/wireguard.rs:102` |
| fn | `as_bytes` | `src/net/wireguard.rs:113` |
| struct | `WgPeer` | `src/net/wireguard.rs:127` |
| struct | `WgSession` | `src/net/wireguard.rs:174` |
| fn | `new` | `src/net/wireguard.rs:199` |
| fn | `is_allowed_ip` | `src/net/wireguard.rs:229` |
| fn | `encrypt_packet` | `src/net/wireguard.rs:253` |
| fn | `decrypt_packet` | `src/net/wireguard.rs:297` |
| struct | `WgDevice` | `src/net/wireguard.rs:361` |
| struct | `WgStats` | `src/net/wireguard.rs:382` |
| fn | `new` | `src/net/wireguard.rs:393` |
| fn | `add_peer` | `src/net/wireguard.rs:412` |
| fn | `remove_peer` | `src/net/wireguard.rs:420` |
| fn | `get_peer` | `src/net/wireguard.rs:425` |
| fn | `find_peer_by_ip` | `src/net/wireguard.rs:430` |
| fn | `initiate_handshake` | `src/net/wireguard.rs:451` |
| fn | `process_message` | `src/net/wireguard.rs:473` |
| fn | `send_keepalive` | `src/net/wireguard.rs:660` |
| struct | `WgManager` | `src/net/wireguard.rs:714` |
| const | `fn` | `src/net/wireguard.rs:721` |
| fn | `create_device` | `src/net/wireguard.rs:728` |
| fn | `delete_device` | `src/net/wireguard.rs:739` |
| fn | `get_device` | `src/net/wireguard.rs:744` |
| struct | `WgRuntimeStatus` | `src/net/wireguard.rs:754` |
| fn | `runtime_status` | `src/net/wireguard.rs:760` |
| enum | `WgError` | `src/net/wireguard.rs:786` |
| fn | `init` | `src/net/wireguard.rs:806` |

## A21 - HPACK Huffman decode fail-closed modeli

- Dosya: `src/net/http2_huffman.rs`
- Public sembol sayisi: 2

| Tip | Sembol | Konum |
|---|---|---|
| enum | `HuffmanDecodeError` | `src/net/http2_huffman.rs:5` |
| fn | `decode_huffman` | `src/net/http2_huffman.rs:144` |

---

Toplam listelenen public sembol sayisi: **801**
