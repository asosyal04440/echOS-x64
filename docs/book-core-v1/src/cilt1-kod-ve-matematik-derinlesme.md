# Cilt 1 Kod ve Matematik Derinlesme

Bu bolumde her cekirdek alt sistem icin iki sey birlikte verilir: (1) dogrudan kod parcasi, (2) karar/maliyet modelini aciklayan matematiksel cerceve.

Matematik burada formel kanit iddiasi degil, muhendislik kararinin hesaplanabilir ozetidir.

---

## KM01 - Boot ve erken init: state publication

Kaynak dosya: `src/main.rs`

### Kod parcasi

```rust
const COM1: u16 = 0x3F8;
const BOOT_MAGIC_UEFI: u64 = 0x55454649;
const BOOT_MAGIC_MB2: u64 = 0x36d76289;
const CMDLINE_MAX_LEN: usize = 4096;
const SECURE_BOOT_ENROLL_MAGIC: u32 = 0x5342_4531;
const SECURE_BOOT_ENROLL_PENDING_RESET: u8 = 1 << 0;
const SECURE_BOOT_ENROLL_FAILED: u8 = 1 << 1;
const LIMINE_REVISION: u64 = 4;
struct SecureBootEnrollState {
unsafe fn inb(port: u16) -> u8 {
fn serial_write_byte(byte: u8) {
struct SerialPort;
impl Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
fn serial_write_str(args: &fmt::Arguments) {
fn init_platform_iommu() -> bool {
fn parse_swap_cmdline(cmdline: &str) -> Option<(u32, u32)> {
fn panic(info: &core::panic::PanicInfo) -> ! {
pub extern "system" fn mainCRTStartup() -> ! {
pub extern "system" fn WinMainCRTStartup() -> ! {
pub extern "C" fn main() -> i32 {
pub extern "C" fn memcpy(dest: *mut u8, src: *const u8, len: usize) -> *mut u8 {
pub extern "C" fn memmove(dest: *mut u8, src: *const u8, len: usize) -> *mut u8 {
pub extern "C" fn memset(dest: *mut u8, value: i32, len: usize) -> *mut u8 {
pub extern "C" fn memcmp(lhs: *const u8, rhs: *const u8, len: usize) -> i32 {
pub extern "C" fn strlen(ptr: *const u8) -> usize {
pub extern "C" fn fmod(x: f64, y: f64) -> f64 {
pub extern "C" fn fmodf(x: f32, y: f32) -> f32 {
pub extern "system" fn __CxxFrameHandler3() -> i32 {
const PREFERRED_GOP_WIDTH: usize = 1920;
const PREFERRED_GOP_HEIGHT: usize = 1080;
fn gop_mode_rank(width: usize, height: usize, target_width: usize, target_height: usize) -> u8 {
fn configure_preferred_gop_mode(gop: &mut GraphicsOutput) {
pub extern "C" fn kernel_entry(boot_info_addr: usize, kaslr_offset: u64, boot_magic: u64) -> ! {
pub extern "C" fn kernel_main(boot_info_addr: usize, kaslr_offset: u64, boot_magic: u64) -> ! {
fn limine_available() -> bool {
unsafe fn boot_pipeline_uefi(boot_info_addr: usize, _kaslr_offset: u64) -> ! {
unsafe fn boot_pipeline_limine(kaslr_offset: u64) -> ! {
unsafe fn boot_pipeline_multiboot(boot_info_addr: usize, kaslr_offset: u64) -> ! {
pub extern "efiapi" fn efi_main(image: Handle, mut system_table: SystemTable<Boot>) -> Status {
fn detect_secure_boot(system_table: &SystemTable<Boot>) -> bool {
fn read_global_u8_variable(system_table: &SystemTable<Boot>, name: &CStr16) -> Option<u8> {
fn appliance_variable_vendor() -> VariableVendor {
fn read_boot_control_variable_seed(
) -> Option<ech_os::boot::appliance::BootControlBlock> {
fn curated_app_bundle_path(index: u8) -> Option<&'static CStr16> {
fn read_efi_boot_file(
) -> Option<Vec<u8>> {
fn efi_boot_file_size(
) -> Option<usize> {
fn read_boot_control_seed(
) -> Option<ech_os::boot::appliance::BootControlBlock> {
fn sync_boot_control_seed(
fn read_secure_boot_enroll_state(
) -> Option<SecureBootEnrollState> {
fn write_secure_boot_enroll_state(
) -> Result<(), Status> {
fn write_global_variable_payload(
) -> Result<(), Status> {
fn auto_enroll_secure_boot_payloads(
) -> Result<(), Status> {
fn inspect_loaded_image(
) -> Result<([u8; 32], u64), Status> {
fn measure_loaded_image_tpm(
) -> Result<(), Status> {
fn measure_cmdline_tpm(
) -> Result<(), Status> {
fn report_tpm_event_log(system_table: &mut SystemTable<Boot>) {
fn read_cmdline(
) -> Result<(u64, u64), Status> {
```

### Matematiksel model

\[L_{boot}=L_{fw}+L_{loader}+L_{early\_map}+L_{subsys}\]

\[P_{fail}=1-\prod_i (1-p_i)\]

\[S_{boot}=\frac{1}{1+\sigma_{state}}\]

### Muhendislik yorumu

Bu alt sistemde kod parcasi, state gecislerinin hangi sira ve hangi kosullarla ilerledigini gosteren bir publication haritasi gibi okunur. Denklem seti ise bu haritanin tail-latency, dogruluk ve kaynak maliyeti tarafindaki etkisini nicel olarak ifade eder.

Modelde en kritik nokta, tek bir metriği optimize etmek yerine, p99 gecikme, hata yuzeyi ve kaynak tuketimi arasindaki gerilimi ayni anda gormektir. Bu nedenle kararlar satir-bazli kod referansi ile desteklenmeden kabul edilmez.

---

## KM02 - SMP scheduler orkestrasyonu

Kaynak dosya: `src/task/scheduler.rs`

### Kod parcasi

```rust
const MAX_CPUS: usize = 8192;
const MSR_GS_BASE: u32 = 0xC000_0101;
pub struct SmpScheduler {
impl SmpScheduler {
    pub fn new(cpu_count: u32) -> Self {
    pub fn allocate_task_id(&self) -> TaskId {
    pub fn spawn(&self, task: Task) {
    pub fn spawn_boxed(&self, task: Box<Task>) {
fn task_can_run_on_cpu(task: &Task, cpu_id: u32) -> bool {
fn queued_task_count_usize(cpu_id: usize) -> u32 {
pub fn queued_task_count(cpu_id: u32) -> u32 {
fn publish_worker_load(cpu_id: usize) {
fn choose_spawn_cpu(task: &Task) -> usize {
fn enqueue_boxed_task(target_cpu: usize, task: Box<Task>) -> Option<usize> {
const NICE_0_LOAD: u64 = 1024;
const SCHED_LATENCY_TICKS: u64 = 20;
const MIN_GRANULARITY_TICKS: u64 = 4;
const LOAD_BALANCE_INTERVAL: usize = 100;
const VRUNTIME_NORMALIZE_INTERVAL: usize = 2000;
enum SchedulerPressureClass {
struct SchedulerPressureSnapshot {
pub fn init() {
pub fn get_cpu_load(cpu_id: u32) -> f32 {
pub fn update_cpu_count(cpu_count: u32) {
pub fn enable_secondary_scheduling() {
pub fn secondary_scheduling_active() -> bool {
pub fn current_kernel_stack_top() -> u64 {
pub fn classify_current_kernel_stack_fault(addr: u64) -> Option<&'static str> {
pub fn record_current_stack_pointer(rsp: u64) {
pub fn current_kernel_stack_usage() -> Option<(u64, u64)> {
pub fn current_user_target() -> Option<(u64, u64)> {
pub fn current_win32_thread_state() -> Option<Win32ThreadState> {
unsafe fn read_user_gs_base() -> u64 {
pub fn current_task_id() -> TaskId {
pub fn current_execution_mode() -> Option<ExecutionMode> {
pub fn current_user_page_table() -> Option<PhysFrame> {
pub fn current_address_space() -> Option<Arc<Mutex<crate::memory::AddressSpace>>> {
pub fn task_exists(pid: TaskId) -> bool {
pub fn fork_current_user_task(user_rip: u64, user_rsp: u64) -> Option<TaskId> {
pub fn idle_loop() -> ! {
pub fn spawn(entry_point: fn() -> !) -> TaskId {
pub fn spawn_with_priority(
) -> TaskId {
pub fn spawn_with_priority_in_address_space(
) -> TaskId {
pub fn get_ticks() -> usize {
pub fn is_ready() -> bool {
pub fn tick() {
    const LOAD_UPDATE_INTERVAL: usize = 100;
    const LOAD_UPDATE_INTERVAL: usize = 10;
    const BALANCE_INTERVAL: usize = 10000;
    const BALANCE_INTERVAL: usize = 1000;
fn should_preempt(now: u64) -> bool {
fn scheduler_pressure_snapshot() -> SchedulerPressureSnapshot {
fn calc_time_slice(_cpu_id: u32, weight: u32) -> u64 {
fn update_task_vruntime(task: &mut Task, delta_ticks: u64) {
fn wake_sleeping_tasks(_current_tick: usize) {
pub fn sleep(ticks: usize) {
pub fn exit(code: i32) -> ! {
pub fn wait_for_terminated(pid: isize) -> Option<(TaskId, i32)> {
pub fn get_current_ptrace_flags() -> u32 {
pub fn set_ptrace_flag(flag: u32) {
pub fn get_current_seccomp_mode() -> u32 {
pub fn set_current_seccomp_mode(mode: u32) {
pub fn exec_current_user_image(image: &[u8]) -> Result<(), ()> {
pub fn spawn_user_image_task(
) -> Result<TaskId, ()> {
pub fn spawn_user_image_task_with_address_space(
) -> Result<(TaskId, Arc<Mutex<crate::memory::AddressSpace>>), ()> {
fn get_current_cpu_id() -> u32 {
fn has_schedulable_work(cpu_id: u32) -> bool {
fn choose_victim_cpu(cpu_id: u32) -> Option<usize> {
fn restore_worker_tasks(cpu_index: usize, mut deferred: Vec<Box<Task>>) {
fn take_task_from_worker_by_id(cpu_index: usize, task_id: TaskId) -> Option<Box<Task>> {
fn steal_task_from_victim_by_id(victim: usize, task_id: TaskId) -> Option<Box<Task>> {
fn take_committed_policy_task(cpu_id: u32, now_tick: u64) -> Option<Box<Task>> {
pub fn schedule() {
    fn switch_context(old: *mut TaskContext, new: *const TaskContext, fpu_mode: u64);
pub struct TaskInfo {
pub fn list_tasks() -> Vec<TaskInfo> {
pub fn kill_task(pid: TaskId, signal: i32) -> Result<(), &'static str> {
pub fn stop_task(pid: TaskId) {
pub fn continue_task(pid: TaskId) {
pub fn background_current() -> Option<TaskId> {
pub fn get_foreground_task() -> Option<TaskId> {
pub fn foreground_task(pid: TaskId) -> Result<(), &'static str> {
pub fn get_task_state(pid: TaskId) -> Option<TaskState> {
pub fn get_cpu_count() -> u32 {
pub fn steal_from_cpu(cpu_id: u32) -> Option<Box<Task>> {
pub fn push_to_cpu(cpu_id: u32, task: Box<Task>) {
```

### Matematiksel model

\[Skew=\max_i q_i-\min_i q_i\]

\[W_i=\alpha q_i+\beta u_i+\gamma m_i\]

\[J_{p99}=\operatorname*{argmin}_{policy} \; tail(policy)\]

### Muhendislik yorumu

Bu alt sistemde kod parcasi, state gecislerinin hangi sira ve hangi kosullarla ilerledigini gosteren bir publication haritasi gibi okunur. Denklem seti ise bu haritanin tail-latency, dogruluk ve kaynak maliyeti tarafindaki etkisini nicel olarak ifade eder.

Modelde en kritik nokta, tek bir metriği optimize etmek yerine, p99 gecikme, hata yuzeyi ve kaynak tuketimi arasindaki gerilimi ayni anda gormektir. Bu nedenle kararlar satir-bazli kod referansi ile desteklenmeden kabul edilmez.

---

## KM03 - RT scheduler: bandwidth ve dilim kontrolu

Kaynak dosya: `src/task/rt_scheduler.rs`

### Kod parcasi

```rust
pub const RT_PRIO_MIN: i32 = 1;
pub const RT_PRIO_MAX: i32 = 99;
pub const RR_DEFAULT_TIMESLICE: u64 = 100;
pub const RR_MAX_TIMESLICE: u64 = 200;
pub const RR_MIN_TIMESLICE: u64 = 10;
pub enum SchedPolicy {
impl Default for SchedPolicy {
    fn default() -> Self {
pub struct RtSchedParam {
impl Default for RtSchedParam {
    fn default() -> Self {
pub struct RtTaskInfo {
impl RtTaskInfo {
    pub fn new(task_id: TaskId) -> Self {
    pub fn with_rt(task_id: TaskId, policy: SchedPolicy, priority: i32) -> Self {
    fn calculate_timeslice(priority: i32) -> u64 {
    pub fn reset_timeslice(&mut self) {
    pub fn tick(&mut self) -> bool {
pub struct RtRunQueue {
impl RtRunQueue {
    pub fn new() -> Self {
    pub fn enqueue(&mut self, task: Box<Task>) {
    pub fn dequeue(&mut self, task_id: TaskId) -> Option<Box<Task>> {
    pub fn pick_next(&mut self) -> Option<Box<Task>> {
    fn find_highest_prio(&self) -> i32 {
    fn update_highest_prio(&mut self) {
    pub fn rt_task_count(&self) -> u64 {
    pub fn has_rt_tasks(&self) -> bool {
    pub fn set_sched_param(&mut self, task_id: TaskId, policy: SchedPolicy, param: &RtSchedParam) {
    pub fn get_sched_param(&self, task_id: TaskId) -> Option<(SchedPolicy, RtSchedParam)> {
    pub fn tick(&mut self, task_id: TaskId) -> bool {
    pub fn reenqueue_rr(&mut self, task: Box<Task>) {
    pub fn set_rt_bandwidth(&mut self, runtime: u64, period: u64) {
    pub fn set_rt_throttling(&mut self, enabled: bool) {
impl Default for RtRunQueue {
    fn default() -> Self {
pub fn init() {
pub fn has_rt_tasks() -> bool {
pub fn rt_task_count() -> u64 {
pub fn enqueue_rt_task(task: Box<Task>) {
pub fn dequeue_rt_task(task_id: TaskId) -> Option<Box<Task>> {
pub fn pick_next_rt_task() -> Option<Box<Task>> {
pub fn set_sched_param(task_id: TaskId, policy: SchedPolicy, param: &RtSchedParam) {
pub fn get_sched_param(task_id: TaskId) -> Option<(SchedPolicy, RtSchedParam)> {
pub fn rt_tick(task_id: TaskId) -> bool {
pub fn reenqueue_rr_task(task: Box<Task>) {
pub fn set_rt_bandwidth(runtime: u64, period: u64) {
pub fn set_rt_throttling(enabled: bool) {
pub fn is_rt_task(task_id: TaskId) -> bool {
pub fn get_task_priority(task_id: TaskId) -> i32 {
pub fn get_task_policy(task_id: TaskId) -> SchedPolicy {
pub fn yield_rt_task(task: Box<Task>) {
pub fn sys_sched_setscheduler(task_id: TaskId, policy: i32, param: &RtSchedParam) -> i32 {
pub fn sys_sched_getscheduler(task_id: TaskId) -> i32 {
pub fn sys_sched_setparam(task_id: TaskId, param: &RtSchedParam) -> i32 {
pub fn sys_sched_getparam(task_id: TaskId) -> Option<RtSchedParam> {
pub fn sys_sched_yield() {
pub fn sys_sched_get_priority_max(policy: i32) -> i32 {
pub fn sys_sched_get_priority_min(policy: i32) -> i32 {
pub fn sys_sched_rr_get_interval(task_id: TaskId) -> u64 {
```

### Matematiksel model

\[U_{rt}=\sum_i \frac{C_i}{T_i}\]

\[U_{rt}\le U_{cap}\]

\[Q_{rr}(p)=\operatorname{clip}(Q_{min},Q_{max},f(p))\]

### Muhendislik yorumu

Bu alt sistemde kod parcasi, state gecislerinin hangi sira ve hangi kosullarla ilerledigini gosteren bir publication haritasi gibi okunur. Denklem seti ise bu haritanin tail-latency, dogruluk ve kaynak maliyeti tarafindaki etkisini nicel olarak ifade eder.

Modelde en kritik nokta, tek bir metriği optimize etmek yerine, p99 gecikme, hata yuzeyi ve kaynak tuketimi arasindaki gerilimi ayni anda gormektir. Bu nedenle kararlar satir-bazli kod referansi ile desteklenmeden kabul edilmez.

---

## KM04 - CFS: vruntime geometrisi

Kaynak dosya: `src/task/cfs.rs`

### Kod parcasi

```rust
pub const CFS_DEFAULT_SLICE: u64 = 1_000_000; // 1ms
pub const CFS_MIN_GRANULARITY: u64 = 1_000_000; // 1ms
pub const CFS_WAKEUP_GRANULARITY: u64 = 1_000_000;
pub const CFS_NICE_0_WEIGHT: u64 = 1024;
pub const CFS_LOAD_AVG_PERIOD: u64 = 32;
pub const CFS_PELT_HALF_LIFE: u64 = 32; // 32ms
pub fn nice_to_weight(nice: i32) -> u64 {
pub fn weight_to_vruntime(delta: u64, weight: u64) -> u64 {
pub struct CfsTask {
pub struct CfsStats {
impl CfsTask {
    pub fn new(task_id: u64, nice: i32) -> Self {
    pub fn set_nice(&self, nice: i32) {
    pub fn update_vruntime(&self, delta: u64) {
    pub fn calc_slice(&self, total_weight: u64, nr_running: u64) -> u64 {
    pub fn is_eligible(&self, min_vruntime: u64) -> bool {
pub struct CfsRq {
impl CfsRq {
    pub fn new() -> Self {
    pub fn enqueue(&self, task: Arc<CfsTask>) {
    pub fn dequeue(&self, task: &CfsTask) {
    pub fn pick_next(&self) -> Option<Arc<CfsTask>> {
    pub fn put_prev(&self, task: &CfsTask) {
    pub fn update_clock(&self, now: u64) {
    pub fn update_load_avg(&self, task: &CfsTask, delta: u64) {
    pub fn check_preempt_wakeup(&self, task: &CfsTask) -> bool {
pub struct CfsScheduler {
impl CfsScheduler {
    pub fn new(nr_cpus: usize) -> Self {
    pub fn schedule(&self, cpu: usize) -> Option<Arc<CfsTask>> {
    pub fn tick(&self, cpu: usize) {
    pub fn enqueue(&self, task: Arc<CfsTask>, cpu: usize) {
    pub fn dequeue(&self, task: &CfsTask, cpu: usize) {
    pub fn load_balance(&self) {
    pub fn set_nice(&self, task: &CfsTask, nice: i32) {
pub fn sys_sched_setparam(pid: u64, nice: i32) -> i32 {
pub fn sys_sched_getparam(pid: u64) -> i32 {
pub fn sys_sched_yield() -> i32 {
pub fn init() {
```

### Matematiksel model

\[\Delta v = \frac{\Delta t\,W_0}{w_i}\]

\[v_i(t+1)=v_i(t)+\Delta v_i\]

\[Fairness\;Gap=\max_i v_i-\min_i v_i\]

### Muhendislik yorumu

Bu alt sistemde kod parcasi, state gecislerinin hangi sira ve hangi kosullarla ilerledigini gosteren bir publication haritasi gibi okunur. Denklem seti ise bu haritanin tail-latency, dogruluk ve kaynak maliyeti tarafindaki etkisini nicel olarak ifade eder.

Modelde en kritik nokta, tek bir metriği optimize etmek yerine, p99 gecikme, hata yuzeyi ve kaynak tuketimi arasindaki gerilimi ayni anda gormektir. Bu nedenle kararlar satir-bazli kod referansi ile desteklenmeden kabul edilmez.

---

## KM05 - EEVDF: eligibility ve virtual deadline

Kaynak dosya: `src/task/eevdf.rs`

### Kod parcasi

```rust
pub struct EevdfTask {
impl EevdfTask {
    pub fn new(task_id: u64, weight: u64, slice_ns: u64) -> Self {
    pub fn update_runtime(&self, delta_ns: u64, rq_vtime: u64) {
struct DeadlineKey {
pub struct EevdfStats {
pub struct EevdfRunQueue {
impl EevdfRunQueue {
    pub fn new() -> Self {
    pub fn vtime(&self) -> u64 {
    pub fn enqueue(&self, task: Arc<EevdfTask>) {
    pub fn dequeue(&self, task_id: u64) -> Option<Arc<EevdfTask>> {
    pub fn account_runtime(&self, task_id: u64, delta_ns: u64) {
    pub fn pick_next(&self) -> Option<Arc<EevdfTask>> {
    pub fn should_preempt(&self, current_task_id: u64, wakee_task_id: u64) -> bool {
    pub fn stats(&self) -> EevdfStats {
    pub fn ordered_task_ids(&self) -> Vec<u64> {
```

### Matematiksel model

\[lag_i = service_i - fair_i\]

\[eligible_i = [lag_i\ge 0]\]

\[vd_i = vtime + \frac{slice_i}{w_i}\]

### Muhendislik yorumu

Bu alt sistemde kod parcasi, state gecislerinin hangi sira ve hangi kosullarla ilerledigini gosteren bir publication haritasi gibi okunur. Denklem seti ise bu haritanin tail-latency, dogruluk ve kaynak maliyeti tarafindaki etkisini nicel olarak ifade eder.

Modelde en kritik nokta, tek bir metriği optimize etmek yerine, p99 gecikme, hata yuzeyi ve kaynak tuketimi arasindaki gerilimi ayni anda gormektir. Bu nedenle kararlar satir-bazli kod referansi ile desteklenmeden kabul edilmez.

---

## KM06 - Deadline scheduler: admission denklemi

Kaynak dosya: `src/task/deadline.rs`

### Kod parcasi

```rust
pub const DL_DEFAULT_RUNTIME: u64 = 100_000; // 100ms
pub const DL_DEFAULT_PERIOD: u64 = 1_000_000; // 1s
pub const DL_DEFAULT_DEADLINE: u64 = DL_DEFAULT_PERIOD;
pub const SCHED_DEADLINE: i32 = 6;
pub const SCHED_FLAG_DL_OVERRUN: u64 = 1 << 0;  // Bütçe aşımı bildirimi iste
pub const SCHED_FLAG_DL_RECLAIM: u64 = 1 << 1;  // Boşta kalan bant genişliğini geri al
pub const SCHED_FLAG_DL_SPECIAL: u64 = 1 << 2;  // Özel EDF varyantı
pub struct DeadlineTask {
pub struct DlStats {
impl DeadlineTask {
    pub fn new(task_id: u64, runtime: u64, period: u64, deadline: u64, flags: u64) -> Self {
    pub fn deadline_passed(&self) -> bool {
    pub fn runtime_exhausted(&self) -> bool {
    pub fn consume_runtime(&self, ns: u64) {
    pub fn replenish(&self) {
    pub fn laxity(&self) -> i64 {
    pub fn compare_deadline(&self, other: &DeadlineTask) -> core::cmp::Ordering {
pub struct DeadlineRq {
impl DeadlineRq {
    pub fn new() -> Self {
    pub fn enqueue(&self, task: Arc<DeadlineTask>) -> Result<(), DlError> {
    pub fn dequeue(&self, task: &DeadlineTask) {
    pub fn pick_next(&self) -> Option<Arc<DeadlineTask>> {
    fn compute_bandwidth(&self, task: &DeadlineTask) -> u64 {
    pub fn check_replenishments(&self) {
    pub fn check_deadline_misses(&self) {
pub struct DeadlineScheduler {
impl DeadlineScheduler {
    pub fn new(nr_cpus: usize) -> Self {
    pub fn schedule(&self, cpu: usize) -> Option<Arc<DeadlineTask>> {
    pub fn add_task(&self, task: Arc<DeadlineTask>, cpu: usize) -> Result<(), DlError> {
    pub fn remove_task(&self, task: &DeadlineTask, cpu: usize) {
    pub fn tick(&self, cpu: usize) {
    pub fn set_bandwidth_cap(&self, cap: u64) {
pub enum DlError {
pub fn sys_sched_setattr(pid: u64, runtime: u64, period: u64, deadline: u64, flags: u64) -> i32 {
pub fn sys_sched_getattr(pid: u64, attr: &mut SchedAttr) -> i32 {
pub struct SchedAttr {
pub fn init() {
```

### Matematiksel model

\[U=\sum_i \frac{C_i}{T_i}\]

\[U\le 1-\epsilon\]

\[R_i=C_i+\sum_{j\ne i}\left\lceil\frac{R_i}{T_j}\right\rceil C_j\]

### Muhendislik yorumu

Bu alt sistemde kod parcasi, state gecislerinin hangi sira ve hangi kosullarla ilerledigini gosteren bir publication haritasi gibi okunur. Denklem seti ise bu haritanin tail-latency, dogruluk ve kaynak maliyeti tarafindaki etkisini nicel olarak ifade eder.

Modelde en kritik nokta, tek bir metriği optimize etmek yerine, p99 gecikme, hata yuzeyi ve kaynak tuketimi arasindaki gerilimi ayni anda gormektir. Bu nedenle kararlar satir-bazli kod referansi ile desteklenmeden kabul edilmez.

---

## KM07 - Chase-Lev deque: lock-free yarismalar

Kaynak dosya: `src/task/deque.rs`

### Kod parcasi

```rust
const DEQUE_SIZE: usize = 4096;
pub struct Worker<T> {
pub struct Stealer<T> {
struct Inner<T> {
impl<T> Worker<T> {
    pub fn new() -> (Worker<T>, Stealer<T>) {
    pub fn push(&self, task: Box<T>) {
    pub fn pop(&self) -> Option<Box<T>> {
    pub fn len(&self) -> usize {
impl<T> Stealer<T> {
    pub fn steal(&self) -> Option<Box<T>> {
```

### Matematiksel model

\[P_{race}=P(pop\cap steal\cap single\_slot)\]

\[E[T_{retry}] = \frac{1}{1-p_{cas\_fail}}\]

\[Throughput\approx\frac{ops}{CAS+fence}\]

### Muhendislik yorumu

Bu alt sistemde kod parcasi, state gecislerinin hangi sira ve hangi kosullarla ilerledigini gosteren bir publication haritasi gibi okunur. Denklem seti ise bu haritanin tail-latency, dogruluk ve kaynak maliyeti tarafindaki etkisini nicel olarak ifade eder.

Modelde en kritik nokta, tek bir metriği optimize etmek yerine, p99 gecikme, hata yuzeyi ve kaynak tuketimi arasindaki gerilimi ayni anda gormektir. Bu nedenle kararlar satir-bazli kod referansi ile desteklenmeden kabul edilmez.

---

## KM08 - Timing wheel: amortized analiz

Kaynak dosya: `src/task/timer.rs`

### Kod parcasi

```rust
const WHEEL_SIZE: usize = 256;
const WHEEL_MASK: usize = WHEEL_SIZE - 1;
const WHEEL_BITS: usize = 8; // 2^8 = 256
pub struct TimingWheel {
impl TimingWheel {
    pub fn new(_size: usize) -> Self {
    pub fn schedule(&mut self, mut task: Box<Task>, wake_tick: usize) {
    pub fn tick(&mut self) -> Vec<Box<Task>> {
    fn cascade(&mut self, level: usize, tick: usize) {
```

### Matematiksel model

\[T_{insert}=O(1)\]

\[T_{tick}=O(1)\;\text{(amortized)}\]

\[L_{timer}=L_{bucket}+L_{cascade}\]

### Muhendislik yorumu

Bu alt sistemde kod parcasi, state gecislerinin hangi sira ve hangi kosullarla ilerledigini gosteren bir publication haritasi gibi okunur. Denklem seti ise bu haritanin tail-latency, dogruluk ve kaynak maliyeti tarafindaki etkisini nicel olarak ifade eder.

Modelde en kritik nokta, tek bir metriği optimize etmek yerine, p99 gecikme, hata yuzeyi ve kaynak tuketimi arasindaki gerilimi ayni anda gormektir. Bu nedenle kararlar satir-bazli kod referansi ile desteklenmeden kabul edilmez.

---

## KM09 - Zone-aware PMM

Kaynak dosya: `src/memory/fibonacci_pmm.rs`

### Kod parcasi

```rust
pub enum MemoryZone {
const ZONE_DMA_LIMIT: u64 = 16 * 1024 * 1024; // 16 MB
const ZONE_DMA32_LIMIT: u64 = 4 * 1024 * 1024 * 1024; // 4 GB
impl MemoryZone {
    fn from_addr(addr: u64) -> Self {
    fn fallback(self) -> Option<MemoryZone> {
struct RegionAllocator {
pub struct FibonacciPmm {
impl FibonacciPmm {
    pub fn empty() -> Self {
    fn zone_idx(zone: MemoryZone) -> usize {
    pub fn allocate_from_zone(&mut self, zone: MemoryZone) -> Option<PhysFrame> {
    pub fn allocate_contiguous_from_zone(
    ) -> Option<PhysFrame> {
    fn try_allocate_zone(&mut self, zone: MemoryZone) -> Option<PhysFrame> {
    fn try_allocate_contiguous_zone(
    ) -> Option<PhysFrame> {
    pub fn allocate_frame(&mut self) -> Option<PhysFrame> {
    pub fn allocate_contiguous(&mut self, pages: usize) -> Option<PhysFrame> {
    pub fn deallocate_contiguous(&mut self, start: PhysFrame, pages: usize) {
    pub fn utilization(&self) -> f64 {
    pub fn total_frames(&self) -> usize {
    pub fn free_frames(&self) -> usize {
    pub fn zone_stats(&self, zone: MemoryZone) -> (usize, usize, usize) {
    pub fn fragmentation(&self) -> f64 {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
    fn test_fibonacci_pmm_allocation() {
    fn test_zone_allocation() {
```

### Matematiksel model

\[F_{free}=F_{total}-F_{used}-F_{reserved}\]

\[p(z)=\frac{alloc_z}{\sum_k alloc_k}\]

\[Fallback\;Rate=\frac{fallbacks}{alloc\_req}\]

### Muhendislik yorumu

Bu alt sistemde kod parcasi, state gecislerinin hangi sira ve hangi kosullarla ilerledigini gosteren bir publication haritasi gibi okunur. Denklem seti ise bu haritanin tail-latency, dogruluk ve kaynak maliyeti tarafindaki etkisini nicel olarak ifade eder.

Modelde en kritik nokta, tek bir metriği optimize etmek yerine, p99 gecikme, hata yuzeyi ve kaynak tuketimi arasindaki gerilimi ayni anda gormektir. Bu nedenle kararlar satir-bazli kod referansi ile desteklenmeden kabul edilmez.

---

## KM10 - Fibonacci buddy

Kaynak dosya: `src/memory/fibonacci_buddy.rs`

### Kod parcasi

```rust
const PAGE_SIZE: usize = 4096;
const FIBONACCI_SERIES: [usize; 32] = [
pub struct FibonacciBuddyAllocator {
impl FibonacciBuddyAllocator {
    pub fn new(base: PhysAddr, size: usize) -> Self {
    fn find_fib_index(pages: usize) -> usize {
    fn find_fib_index_floor(pages: usize) -> usize {
    pub fn allocate(&mut self, size: usize) -> Option<PhysAddr> {
    pub fn deallocate(&mut self, addr: PhysAddr, size: usize) {
    fn find_buddy(&self, addr: PhysAddr, idx: usize) -> PhysAddr {
    fn split_block(&mut self, block: PhysAddr, from_idx: usize, to_idx: usize) -> PhysAddr {
    fn try_coalesce(&mut self, addr: PhysAddr, idx: usize) {
    fn find_block_in_freelist(&self, addr: PhysAddr) -> Option<usize> {
    pub fn utilization(&self) -> f64 {
    pub fn fragmentation(&self) -> f64 {
    fn test_fibonacci_allocation() {
    fn test_buddy_coalescing() {
```

### Matematiksel model

\[F_n=F_{n-1}+F_{n-2}\]

\[Frag_{int}=\frac{unused}{allocated}\]

\[Coalesce\;Success=\frac{merge\_ok}{free\_ops}\]

### Muhendislik yorumu

Bu alt sistemde kod parcasi, state gecislerinin hangi sira ve hangi kosullarla ilerledigini gosteren bir publication haritasi gibi okunur. Denklem seti ise bu haritanin tail-latency, dogruluk ve kaynak maliyeti tarafindaki etkisini nicel olarak ifade eder.

Modelde en kritik nokta, tek bir metriği optimize etmek yerine, p99 gecikme, hata yuzeyi ve kaynak tuketimi arasindaki gerilimi ayni anda gormektir. Bu nedenle kararlar satir-bazli kod referansi ile desteklenmeden kabul edilmez.

---

## KM11 - TLSF wrapper

Kaynak dosya: `src/allocator/tlsf.rs`

### Kod parcasi

```rust
const EARLY_HEAP_SIZE: usize = 512 * 1024;
const HEAP_CANARY_MAGIC: u64 = 0xDEADBEEF_CAFEBABE;
const MAX_TRACKED_ALLOCATIONS: usize = 4096;
struct AllocationEntry {
impl AllocationEntry {
    const fn new() -> Self {
pub struct LockedTlsf(Mutex<Option<Tlsf<'static, usize, usize, 32, 32>>>);
impl LockedTlsf {
    pub const fn new() -> Self {
    fn is_early_heap(ptr: usize) -> bool {
    fn is_main_heap(ptr: usize) -> bool {
    fn is_valid_heap_ptr(ptr: usize) -> bool {
    pub fn check_integrity() -> IntegrityReport {
    pub fn corruption_count() -> usize {
    pub fn check_heap_integrity() -> usize {
    pub fn get_stats() -> AllocStats {
    pub fn memory_stats() -> MemoryStats {
    pub unsafe fn alloc_from_main_heap(&self, layout: Layout) -> *mut u8 {
pub struct IntegrityReport {
pub struct MemoryStats {
pub struct AllocStats {
fn early_alloc(layout: Layout) -> *mut u8 {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
pub fn heap_stats() -> (usize, usize) {
pub fn early_heap_usage() -> usize {
pub fn main_heap_bounds() -> (usize, usize) {
```

### Matematiksel model

\[T_{alloc}=O(1)\]

\[B_{bucket}=2^{fli}\cdot(1+\frac{sli}{N_{sli}})\]

\[P_{corrupt}\propto P(check\_skip)\]

### Muhendislik yorumu

Bu alt sistemde kod parcasi, state gecislerinin hangi sira ve hangi kosullarla ilerledigini gosteren bir publication haritasi gibi okunur. Denklem seti ise bu haritanin tail-latency, dogruluk ve kaynak maliyeti tarafindaki etkisini nicel olarak ifade eder.

Modelde en kritik nokta, tek bir metriği optimize etmek yerine, p99 gecikme, hata yuzeyi ve kaynak tuketimi arasindaki gerilimi ayni anda gormektir. Bu nedenle kararlar satir-bazli kod referansi ile desteklenmeden kabul edilmez.

---

## KM12 - Page fault, COW ve THP

Kaynak dosya: `src/memory/mod.rs`

### Kod parcasi

```rust
pub struct MemoryStats {
pub const KERNEL_HEAP_BASE: u64 = crate::allocator::HEAP_START as u64;
pub const KERNEL_HEAP_SIZE: usize = crate::allocator::HEAP_SIZE;
pub fn get_memory_stats() -> MemoryStats {
pub struct MemoryManager {
impl MemoryManager {
    pub fn new(memory_map: MemoryMap<'static>) -> Self {
    pub fn get_memory_map(&self) -> MemoryMapIter<'_> {
    pub fn memory_map_mut(&mut self) -> &mut MemoryMap<'static> {
    pub fn allocate_contiguous_frames(&mut self, pages: usize) -> Option<PhysFrame> {
    pub fn deallocate_contiguous_frames(&mut self, start: PhysFrame, pages: usize) {
    pub fn total_frames(&self) -> usize {
    pub fn free_frames(&self) -> usize {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
pub fn init_uefi(memory_map: MemoryMap<'static>) -> MemoryManager {
pub fn init_memory_subsystems() {
pub unsafe fn global_memory_manager_mut() -> Option<&'static mut MemoryManager> {
pub fn global_memory_manager() -> Option<&'static MemoryManager> {
) -> Option<&'static mut frame_allocator::Multiboot2FrameAllocator> {
pub const PAGE_SIZE: usize = 4096;
pub const PHYSICAL_MEMORY_OFFSET: u64 = 0xFFFF_8000_0000_0000;
pub const KERNEL_SPACE_START: u64 = 0xFFFF_FFFF_8000_0000;
pub const KERNEL_STACK_VIRT_BASE: u64 = 0xFFFF_FE00_0000_0000;
pub const KERNEL_STACK_VIRT_LIMIT: u64 = 0xFFFF_FE80_0000_0000;
pub const USER_SPACE_START: u64 = 0x0000_0000_0000_0000;
pub const USER_SPACE_END: u64 = 0x0000_7fff_ffff_ffff;
pub const USER_STACK_TOP: u64 = 0x0000_7fff_ffff_f000;
pub const USER_STACK_PAGES: usize = 16;
pub const USER_HEAP_BASE: u64 = 0x0000_1000_0000;
pub const USER_MMAP_BASE: u64 = 0x0000_4000_0000;
pub const USER_MMAP_RANDOM_RANGE: u64 = 1024 * 1024 * 1024 * 1024;
pub const USER_STACK_RANDOM_RANGE: u64 = 256 * 1024 * 1024 * 1024;
pub const USER_HEAP_RANDOM_RANGE: u64 = 1024 * 1024 * 1024;
const RECLAIM_HIGH_DIV: usize = 5;
const RECLAIM_LOW_DIV: usize = 10;
const RECLAIM_MIN_HIGH: usize = 128;
const RECLAIM_MIN_LOW: usize = 64;
const KSWAPD_SLEEP_TICKS: usize = 50;
const KSWAPD_RECLAIM_BATCH: usize = 128;
const WRITEBACK_BUDGET_FAST: usize = 16;
const WRITEBACK_BUDGET_IDLE: usize = 4;
const DIRTY_BG_DIV: usize = 20;
const DIRTY_LIMIT_DIV: usize = 10;
const DIRTY_INODE_LIMIT: usize = 128;
const WRITEBACK_TOKENS_PER_TICK: usize = 16;
const WRITEBACK_INODE_TOKENS_PER_TICK: usize = 4;
const WRITEBACK_TOKEN_CAP: usize = 256;
const WRITEBACK_INODE_TOKEN_CAP: usize = 64;
const THP_PAGES: usize = 512;
enum VmaKind {
struct Vma {
struct ImageRef {
struct PageCacheEntry {
struct FrameRefCounts {
struct SharedAnonPages {
struct SharedFilePages {
struct PageCache {
enum PageBacking {
struct LruEntry {
enum LruClass {
struct SpaceLruCounts {
struct LruState {
impl LruState {
    fn new() -> Self {
    fn touch(&mut self, entry: LruEntry) {
    fn pop_oldest_balanced(
    ) -> Option<LruEntry> {
    fn remove_page(&mut self, space_id: u64, page_index: u64) {
    fn record_refault(&mut self, space_id: u64, page_index: u64) {
    fn pop_matching(
    ) -> Option<LruEntry> {
    fn class_of(entry: &LruEntry) -> LruClass {
    fn adjust_counts(&mut self, entry: &LruEntry, add: bool) {
struct SwapState {
struct SwapDeviceState {
struct WritebackEntry {
struct WritebackQueue {
struct DirtyThrottleState {
impl SwapState {
    fn new() -> Self {
    fn insert(&mut self, space_id: u64, page_index: u64, data: Vec<u8>) {
    fn take(&mut self, space_id: u64, page_index: u64) -> Option<Vec<u8>> {
    fn remove(&mut self, space_id: u64, page_index: u64) {
impl SwapDeviceState {
    fn new(device: Box<dyn BlockDevice>, base_lba: u32, max_slots: u32) -> Self {
    fn sector_per_page() -> u32 {
    fn store(&mut self, space_id: u64, page_index: u64, data: &[u8]) -> bool {
    fn take(&mut self, space_id: u64, page_index: u64) -> Option<Vec<u8>> {
    fn remove(&mut self, space_id: u64, page_index: u64) {
impl WritebackQueue {
```

### Matematiksel model

\[T_{fault}=T_{walk}+T_{policy}+T_{map}\]

\[Gain_{thp}=Hit_{tlb}^{2M}-Hit_{tlb}^{4K}\]

\[Cost_{cow}=P(write\_shared)\cdot C_{copy}\]

### Muhendislik yorumu

Bu alt sistemde kod parcasi, state gecislerinin hangi sira ve hangi kosullarla ilerledigini gosteren bir publication haritasi gibi okunur. Denklem seti ise bu haritanin tail-latency, dogruluk ve kaynak maliyeti tarafindaki etkisini nicel olarak ifade eder.

Modelde en kritik nokta, tek bir metriği optimize etmek yerine, p99 gecikme, hata yuzeyi ve kaynak tuketimi arasindaki gerilimi ayni anda gormektir. Bu nedenle kararlar satir-bazli kod referansi ile desteklenmeden kabul edilmez.

---

## KM13 - MGLRU ve zswap

Kaynak dosya: `src/memory/mglru.rs`

### Kod parcasi

```rust
const MGLRU_GENERATIONS: u64 = 8;
const HOT_REF_THRESHOLD: u16 = 3;
const COLD_EVICTION_AGE: u64 = 2;
pub struct MglruPageKey {
pub struct MglruVictim {
struct MglruEntry {
pub struct MglruStats {
struct MglruState {
impl MglruState {
    fn new() -> Self {
    fn generation_slot(generation: u64) -> u64 {
    fn detach_from_generation(&mut self, key: (u64, u64), generation: u64) {
    fn attach_to_generation(&mut self, key: (u64, u64), generation: u64) {
    fn set_generation(&mut self, key: (u64, u64), new_generation: u64, now_tick: u64) {
    fn on_access(&mut self, key: MglruPageKey, node_id: u16, accessed_bit: bool, now_tick: u64) {
    fn age_tick(&mut self, now_tick: u64) {
    fn remove_page(&mut self, key: MglruPageKey) {
    fn record_refault(&mut self, key: MglruPageKey, now_tick: u64) {
    fn record_eviction(&mut self, key: MglruPageKey) {
    fn pick_victim(&self, space_hint: Option<u64>, node_hint: Option<u16>) -> Option<MglruVictim> {
    fn stats(&self) -> MglruStats {
pub fn init(enabled: bool) {
pub fn is_enabled() -> bool {
pub fn record_page_access(
pub fn age_generations(now_tick: u64) {
pub fn record_refault(space_id: u64, page_index: u64, now_tick: u64) {
pub fn record_eviction(space_id: u64, page_index: u64) {
pub fn remove_page(space_id: u64, page_index: u64) {
pub fn pick_victim(space_hint: Option<u64>, node_hint: Option<u16>) -> Option<MglruVictim> {
pub fn get_stats() -> MglruStats {
```

### Matematiksel model

\[\rho=\frac{\lambda_{dirty}}{\mu_{writeback}}\]

\[Refault\;Ratio=\frac{refault}{evict}\]

\[Score(page)=a\,gen+b\,hot-c\,io\_cost\]

### Muhendislik yorumu

Bu alt sistemde kod parcasi, state gecislerinin hangi sira ve hangi kosullarla ilerledigini gosteren bir publication haritasi gibi okunur. Denklem seti ise bu haritanin tail-latency, dogruluk ve kaynak maliyeti tarafindaki etkisini nicel olarak ifade eder.

Modelde en kritik nokta, tek bir metriği optimize etmek yerine, p99 gecikme, hata yuzeyi ve kaynak tuketimi arasindaki gerilimi ayni anda gormektir. Bu nedenle kararlar satir-bazli kod referansi ile desteklenmeden kabul edilmez.

---

## KM14 - zswap core

Kaynak dosya: `src/memory/zswap.rs`

### Kod parcasi

```rust
pub const ZSWAP_MAX_POOL_PERCENT: u32 = 100;
pub const ZSWAP_DEFAULT_POOL_PERCENT: u32 = 20;
pub const ZSWAP_MAX_ZBUD_PAGES: u64 = 1000000;
pub const ZSWAP_DEFAULT_COMPRESSOR: &str = "lz4";
    fn compress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError>;
    fn decompress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError>;
    fn name(&self) -> &'static str;
pub struct Lz4Compressor;
impl Compressor for Lz4Compressor {
    fn compress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError> {
    fn decompress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError> {
    fn name(&self) -> &'static str {
pub struct ZstdCompressor;
impl Compressor for ZstdCompressor {
    fn compress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError> {
    fn decompress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError> {
    fn name(&self) -> &'static str {
pub struct ZswapEntry {
impl ZswapEntry {
    pub fn new(
    ) -> Self {
    pub fn compression_ratio(&self) -> f32 {
impl Clone for ZswapEntry {
    fn clone(&self) -> Self {
pub struct ZswapPool {
impl ZswapPool {
    pub fn new(id: u32, compressor: Arc<dyn Compressor>) -> Self {
    pub fn store(&self, swap_offset: u64, data: &[u8]) -> Result<ZswapEntry, ZswapError> {
    pub fn load(&self, swap_offset: u64, data: &mut [u8]) -> Result<(), ZswapError> {
    pub fn remove(&self, swap_offset: u64) -> bool {
    fn alloc_handle(&self, data: &[u8]) -> Result<u64, ZswapError> {
    fn free_handle(&self, handle: u64) {
    fn get_data(&self, handle: u64) -> Result<Vec<u8>, ZswapError> {
    pub fn compression_ratio(&self) -> f32 {
pub struct ZswapStats {
pub struct ZswapManager {
impl ZswapManager {
    pub fn new() -> Self {
    pub fn init(&self, total_memory: u64) {
    pub fn store(&self, swap_offset: u64, data: &[u8]) -> Result<(), ZswapError> {
    pub fn load(&self, swap_offset: u64, data: &mut [u8]) -> Result<(), ZswapError> {
    pub fn invalidate(&self, swap_offset: u64) -> bool {
    pub fn writeback_lru(&self) -> Result<(), ZswapError> {
    pub fn compression_ratio(&self) -> f32 {
    pub fn get_stats(&self) -> ZswapStats {
    pub fn set_max_pool_percent(&self, percent: u32) {
    pub fn set_enabled(&self, enabled: bool) {
pub struct ZramDevice {
pub struct ZramStats {
impl ZramDevice {
    pub fn new(id: u32) -> Self {
    pub fn write(&self, offset: u64, data: &[u8]) -> Result<(), ZswapError> {
    pub fn read(&self, offset: u64, data: &mut [u8]) -> Result<(), ZswapError> {
    pub fn set_size(&self, size: u64) {
    pub fn reset(&self) {
pub enum ZswapError {
pub fn init(total_memory: u64) {
pub fn is_enabled() -> bool {
```

### Matematiksel model

\[CR=\frac{size_{orig}}{size_{comp}}\]

\[Benefit=IO_{saved}-CPU_{comp}\]

\[Hit_{zswap}=\frac{swapin_{hit}}{swapin_{total}}\]

### Muhendislik yorumu

Bu alt sistemde kod parcasi, state gecislerinin hangi sira ve hangi kosullarla ilerledigini gosteren bir publication haritasi gibi okunur. Denklem seti ise bu haritanin tail-latency, dogruluk ve kaynak maliyeti tarafindaki etkisini nicel olarak ifade eder.

Modelde en kritik nokta, tek bir metriği optimize etmek yerine, p99 gecikme, hata yuzeyi ve kaynak tuketimi arasindaki gerilimi ayni anda gormektir. Bu nedenle kararlar satir-bazli kod referansi ile desteklenmeden kabul edilmez.

---

## KM15 - io_uring lock-free publication

Kaynak dosya: `src/posix/io_uring_ring.rs`

### Kod parcasi

```rust
pub struct SendPtr<T>(*const T);
impl<T> SendPtr<T> {
    pub fn new(ptr: *const T) -> Self {
    pub fn as_ptr(&self) -> *const T {
const RING_SIZE: usize = 256;
const RING_MASK: u32 = (RING_SIZE - 1) as u32;
pub struct RingSqe {
impl Default for RingSqe {
    fn default() -> Self {
pub struct RingCqe {
impl Default for RingCqe {
    fn default() -> Self {
pub const IORING_OP_NOP: u8 = 0;
pub const IORING_OP_READV: u8 = 1;
pub const IORING_OP_WRITEV: u8 = 2;
pub const IORING_OP_FSYNC: u8 = 3;
pub const IORING_OP_READ_FIXED: u8 = 4;
pub const IORING_OP_WRITE_FIXED: u8 = 5;
pub const IORING_OP_POLL_ADD: u8 = 6;
pub const IORING_OP_POLL_REMOVE: u8 = 7;
pub const IORING_OP_READ: u8 = 22;
pub const IORING_OP_WRITE: u8 = 23;
pub struct SubmissionRing {
pub struct CompletionRing {
impl SubmissionRing {
    pub const fn new() -> Self {
    pub fn pending_count(&self) -> u32 {
    pub fn is_full(&self) -> bool {
    pub fn is_empty(&self) -> bool {
    pub fn capacity(&self) -> u32 {
    pub fn push(&self, sqe: RingSqe) -> Result<u32, ()> {
    pub fn pop(&self) -> Option<RingSqe> {
    pub fn pop_batch(&self, out: &mut [RingSqe], max_count: usize) -> usize {
impl CompletionRing {
    pub const fn new() -> Self {
    pub fn pending_count(&self) -> u32 {
    pub fn is_full(&self) -> bool {
    pub fn is_empty(&self) -> bool {
    pub fn push(&self, user_data: u64, res: i32, flags: u32) -> Result<(), ()> {
    pub fn pop(&self) -> Option<RingCqe> {
    pub fn pop_batch(&self, out: &mut [RingCqe], max_count: usize) -> usize {
    pub fn drain_overflow(&self) -> u32 {
pub struct LockFreeIoUring {
impl LockFreeIoUring {
    pub const fn new(ring_fd: usize) -> Self {
    pub fn process_submissions(&self) -> usize {
    pub fn completions_available(&self) -> u32 {
    pub fn submissions_pending(&self) -> u32 {
    pub fn cq_overflow_count(&self) -> u32 {
    pub fn sq_dropped_count(&self) -> u32 {
    fn test_sq_push_pop() {
    fn test_cq_push_pop() {
    fn test_ring_full() {
    fn test_batch_pop() {
    fn test_wrapping_arithmetic() {
```

### Matematiksel model

\[Latency_{ring}=L_{submit}+L_{consume}\]

\[Overflow\;Rate=\frac{cq\_overflow}{cq\_events}\]

\[P_{stale}\downarrow\;\text{with Release/Acquire}\]

### Muhendislik yorumu

Bu alt sistemde kod parcasi, state gecislerinin hangi sira ve hangi kosullarla ilerledigini gosteren bir publication haritasi gibi okunur. Denklem seti ise bu haritanin tail-latency, dogruluk ve kaynak maliyeti tarafindaki etkisini nicel olarak ifade eder.

Modelde en kritik nokta, tek bir metriği optimize etmek yerine, p99 gecikme, hata yuzeyi ve kaynak tuketimi arasindaki gerilimi ayni anda gormektir. Bu nedenle kararlar satir-bazli kod referansi ile desteklenmeden kabul edilmez.

---

## KM16 - TLS 1.3 key schedule

Kaynak dosya: `src/net/tls.rs`

### Kod parcasi

```rust
pub const TLS_VERSION_1_3: u16 = 0x0303;
pub enum ContentType {
impl ContentType {
    pub fn from_u8(v: u8) -> Option<Self> {
pub enum HandshakeType {
impl HandshakeType {
    pub fn from_u8(v: u8) -> Option<Self> {
pub enum CipherSuite {
impl CipherSuite {
    pub fn from_u16(v: u16) -> Option<Self> {
    pub fn key_len(&self) -> usize {
    pub fn iv_len(&self) -> usize {
pub enum NamedGroup {
impl NamedGroup {
    pub fn from_u16(v: u16) -> Option<Self> {
pub enum SignatureScheme {
pub enum TlsError {
pub enum AlertLevel {
pub enum AlertDescription {
pub enum TlsState {
pub struct TlsRecordHeader {
impl TlsRecordHeader {
    pub const SIZE: usize = 5;
    pub fn parse(data: &[u8]) -> Result<Self, TlsError> {
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
pub struct HandshakeHeader {
impl HandshakeHeader {
    pub const SIZE: usize = 4;
    pub fn parse(data: &[u8]) -> Result<Self, TlsError> {
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
pub struct KeySchedule {
impl KeySchedule {
    pub fn new() -> Self {
    pub fn derive_handshake_secret(&mut self, shared_secret: &[u8], transcript_hash: &[u8]) {
    pub fn derive_master_secret(&mut self, transcript_hash: &[u8]) {
    fn derive_traffic_secret(
    ) -> [u8; 32] {
    fn hkdf_expand_label(
    ) -> [u8; 32] {
    pub fn client_handshake_traffic_secret(&self) -> Option<&[u8; 32]> {
    pub fn server_handshake_traffic_secret(&self) -> Option<&[u8; 32]> {
    pub fn client_application_traffic_secret(&self) -> Option<&[u8; 32]> {
    pub fn server_application_traffic_secret(&self) -> Option<&[u8; 32]> {
    pub fn server_finished_verify_data(&self, transcript_hash: &[u8]) -> Option<Vec<u8>> {
    pub fn resumption_psk(&self, transcript_hash: &[u8], nonce: &[u8]) -> Option<Vec<u8>> {
impl Default for KeySchedule {
    fn default() -> Self {
pub struct TlsClient {
impl TlsClient {
    pub fn new() -> Self {
    pub fn build_client_hello(&mut self, hostname: &str) -> Vec<u8> {
    pub fn process_server_hello(&mut self, data: &[u8]) -> Result<(), TlsError> {
    pub fn process_encrypted_extensions(&mut self, data: &[u8]) -> Result<(), TlsError> {
    pub fn process_certificate(&mut self, data: &[u8]) -> Result<(), TlsError> {
    pub fn process_certificate_verify(&mut self, data: &[u8]) -> Result<(), TlsError> {
    pub fn process_finished(&mut self, data: &[u8]) -> Result<(), TlsError> {
    pub fn complete_handshake(&mut self) {
    pub fn process_new_session_ticket(&mut self, data: &[u8]) -> Result<(), TlsError> {
    fn cache_new_session_ticket(&mut self, data: &[u8]) -> Result<(), TlsError> {
    pub fn state(&self) -> &TlsState {
    pub fn is_established(&self) -> bool {
    pub fn cipher_suite(&self) -> Option<CipherSuite> {
impl Default for TlsClient {
    fn default() -> Self {
fn expect_handshake_message<'a>(
) -> Result<&'a [u8], TlsError> {
fn parse_signature_scheme(value: u16) -> Option<SignatureScheme> {
fn finished_verify_len(cipher_suite: CipherSuite) -> Option<usize> {
fn has_tls13_downgrade_sentinel(server_random: &[u8; 32]) -> bool {
fn constant_time_eq(lhs: &[u8], rhs: &[u8]) -> bool {
fn build_server_certificate_verify_message(transcript_hash: &[u8; 32]) -> Vec<u8> {
fn parse_tls13_leaf_public_key(
) -> Option<crate::net::x509::X509PublicKey> {
fn verify_tls13_certificate_signature(
) -> bool {
fn parse_tls_rsa_public_key_components(key_data: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
fn trim_der_integer(mut bytes: &[u8]) -> Vec<u8> {
fn ecdsa_der_to_fixed(signature: &[u8], coordinate_len: usize) -> Option<Vec<u8>> {
fn normalize_ecdsa_integer(integer: &[u8], coordinate_len: usize) -> Option<Vec<u8>> {
fn verify_p256_certificate_signature(
) -> bool {
fn verify_p256_certificate_signature(
) -> bool {
fn verify_p384_certificate_signature(
) -> bool {
fn verify_p384_certificate_signature(
) -> bool {
pub fn wrap_record(content_type: ContentType, data: &[u8]) -> Vec<u8> {
pub fn parse_record(data: &[u8]) -> Result<(TlsRecordHeader, Vec<u8>), TlsError> {
pub fn transcript_hash(transcript: &[u8]) -> [u8; 32] {
```

### Matematiksel model

\[secret_{k+1}=HKDF(secret_k, transcript_k)\]

\[P_{forge}\approx 2^{-tag\_bits}\]

\[State\;Drift=\|state_{peer}-state_{local}\|\]

### Muhendislik yorumu

Bu alt sistemde kod parcasi, state gecislerinin hangi sira ve hangi kosullarla ilerledigini gosteren bir publication haritasi gibi okunur. Denklem seti ise bu haritanin tail-latency, dogruluk ve kaynak maliyeti tarafindaki etkisini nicel olarak ifade eder.

Modelde en kritik nokta, tek bir metriği optimize etmek yerine, p99 gecikme, hata yuzeyi ve kaynak tuketimi arasindaki gerilimi ayni anda gormektir. Bu nedenle kararlar satir-bazli kod referansi ile desteklenmeden kabul edilmez.

---

## KM17 - QUIC parser

Kaynak dosya: `src/net/quic.rs`

### Kod parcasi

```rust
pub const QUIC_VERSION_1: u32 = 0x00000001;
const MAX_ACK_RANGES: u64 = 256;
pub enum QuicPacketType {
pub enum QuicFrameType {
pub enum QuicError {
pub struct ConnectionId {
impl ConnectionId {
    pub fn new(data: Vec<u8>) -> Self {
    pub fn random(len: usize) -> Self {
    pub fn len(&self) -> usize {
    pub fn is_empty(&self) -> bool {
    pub fn as_slice(&self) -> &[u8] {
pub enum StreamType {
pub enum StreamState {
pub struct QuicStream {
impl QuicStream {
    pub fn new(id: u64, stream_type: StreamType) -> Self {
    pub fn can_read(&self) -> bool {
    pub fn can_write(&self) -> bool {
    pub fn write(&mut self, data: &[u8]) -> usize {
    pub fn read(&mut self, buf: &mut [u8]) -> usize {
pub enum QuicFrame {
impl QuicFrame {
    pub fn encode(&self) -> Vec<u8> {
    fn encode_varint(buf: &mut Vec<u8>, val: u64) {
    fn decode_varint(data: &[u8], pos: &mut usize) -> Option<u64> {
    pub fn decode(data: &[u8], pos: &mut usize) -> Option<Self> {
pub enum QuicState {
pub enum QuicCryptoLevel {
pub struct QuicKeys {
pub struct QuicConnection {
impl QuicConnection {
    pub fn new(conn_id_len: usize) -> Self {
    pub fn create_stream(&mut self, stream_type: StreamType) -> u64 {
    pub fn get_stream(&self, stream_id: u64) -> Option<&QuicStream> {
    pub fn get_stream_mut(&mut self, stream_id: u64) -> Option<&mut QuicStream> {
    pub fn on_packet(&mut self, data: &[u8]) -> Result<Vec<QuicFrame>, QuicError> {
    fn parse_frames(&mut self, data: &[u8]) -> Result<Vec<QuicFrame>, QuicError> {
    pub fn build_packet(&mut self, frames: &[QuicFrame]) -> Vec<u8> {
    fn encode_varint(buf: &mut Vec<u8>, val: u64) {
pub struct QuicClient {
impl QuicClient {
    pub fn new(server_addr: super::SocketAddr) -> Self {
    pub fn connect(&mut self) -> Result<Vec<u8>, QuicError> {
    pub fn send(&mut self, stream_id: u64, data: &[u8]) -> Result<Vec<u8>, QuicError> {
    pub fn create_stream(&mut self) -> u64 {
pub struct QuicServer {
impl QuicServer {
    pub fn new() -> Self {
    pub fn on_packet(
    ) -> Option<(Vec<u8>, super::SocketAddr)> {
    fn created_stream_starts_open_with_send_window() {
    fn decode_rejects_ack_with_excessive_ranges() {
impl Default for QuicServer {
    fn default() -> Self {
pub fn compute_nonce(iv: &[u8], packet_number: u64) -> [u8; 12] {
pub fn compute_header_protection_mask(hp_key: &[u8], sample: &[u8]) -> [u8; 5] {
fn aes_key_expansion(key: &[u8], schedule: &mut [u32; 60]) {
fn aes_encrypt_block(block: &[u8; 16], schedule: &[u32; 60]) -> [u8; 16] {
fn gf_mul2(a: u8) -> u8 {
fn gf_mul3(a: u8) -> u8 {
pub fn protect_long_header(packet: &mut [u8], hp_key: &[u8]) {
pub fn unprotect_long_header(packet: &mut [u8], hp_key: &[u8]) {
pub fn encrypt_packet_payload(
) -> Vec<u8> {
pub fn decrypt_packet_payload(
) -> Option<Vec<u8>> {
pub struct SentPacket {
pub struct LossRecovery {
pub enum CongestionState {
impl LossRecovery {
    pub fn new() -> Self {
    pub fn on_packet_sent(
    pub fn on_ack_received(&mut self, largest_acked: u64, ack_delay: u64, now: u64) {
    pub fn update_rtt(&mut self, rtt: u64, ack_delay: u64) {
    pub fn detect_lost_packets(&mut self, now: u64) {
    pub fn pto(&self) -> u64 {
    pub fn loss_detection_timeout(&self, now: u64) -> Option<u64> {
    fn earliest_loss_time(&self, now: u64) -> u64 {
    pub fn on_pto_expired(&mut self) {
    fn on_packets_acked(&mut self, _acked: u64) {
    fn on_congestion_event(&mut self) {
    pub fn can_send(&self) -> bool {
    pub fn send_window(&self) -> u64 {
impl Default for LossRecovery {
    fn default() -> Self {
pub fn derive_initial_secret(conn_id: &[u8], is_client: bool) -> QuicKeys {
fn hkdf_extract(salt: &[u8], ikm: &[u8]) -> Vec<u8> {
fn hkdf_expand(prk: &[u8], label: &[u8], len: usize) -> Vec<u8> {
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> Vec<u8> {
```

### Matematiksel model

\[T_{parse}=\sum_i T(frame_i)\]

\[ACK\;Cost=O(n_{ranges})\]

\[Amplification\le A_{max}\]

### Muhendislik yorumu

Bu alt sistemde kod parcasi, state gecislerinin hangi sira ve hangi kosullarla ilerledigini gosteren bir publication haritasi gibi okunur. Denklem seti ise bu haritanin tail-latency, dogruluk ve kaynak maliyeti tarafindaki etkisini nicel olarak ifade eder.

Modelde en kritik nokta, tek bir metriği optimize etmek yerine, p99 gecikme, hata yuzeyi ve kaynak tuketimi arasindaki gerilimi ayni anda gormektir. Bu nedenle kararlar satir-bazli kod referansi ile desteklenmeden kabul edilmez.

---

## KM18 - WireGuard

Kaynak dosya: `src/net/wireguard.rs`

### Kod parcasi

```rust
pub const WG_DEFAULT_PORT: u16 = 51820;
pub const WG_KEY_SIZE: usize = 32;
pub const WG_MSG_INITIATION: u8 = 1;
pub const WG_MSG_RESPONSE: u8 = 2;
pub const WG_MSG_COOKIE_REPLY: u8 = 3;
pub const WG_MSG_TRANSPORT: u8 = 4;
const WG_TRANSPORT_HEADER_LEN: usize = 16;
const WG_TRANSPORT_TAG_LEN: usize = 16;
const WG_NONCE_UNINITIALIZED: u64 = u64::MAX;
pub struct WgKey(pub [u8; WG_KEY_SIZE]);
impl WgKey {
    pub fn new() -> Self {
    pub fn from_bytes(bytes: [u8; WG_KEY_SIZE]) -> Self {
    pub fn generate() -> Self {
    pub fn as_bytes(&self) -> &[u8; WG_KEY_SIZE] {
pub struct WgPeer {
impl Clone for WgPeer {
    fn clone(&self) -> Self {
pub struct WgSession {
impl WgPeer {
    pub fn new(public_key: WgKey) -> Self {
    pub fn is_allowed_ip(&self, ip: u32) -> bool {
    pub fn encrypt_packet(&self, pkt: &[u8]) -> Result<Vec<u8>, WgError> {
    pub fn decrypt_packet(&self, pkt: &[u8]) -> Result<Vec<u8>, WgError> {
pub struct WgDevice {
pub struct WgStats {
impl WgDevice {
    pub fn new(name: &str) -> Self {
    pub fn add_peer(&self, peer: Arc<WgPeer>) {
    pub fn remove_peer(&self, public_key: &WgKey) {
    pub fn get_peer(&self, public_key: &WgKey) -> Option<Arc<WgPeer>> {
    pub fn find_peer_by_ip(&self, ip: u32) -> Option<Arc<WgPeer>> {
    fn select_single_handshake_peer(&self) -> Result<Arc<WgPeer>, WgError> {
    pub fn initiate_handshake(&self, peer: &WgPeer) -> Result<(), WgError> {
    pub fn process_message(
    ) -> Result<Vec<u8>, WgError> {
    fn process_initiation(&self, pkt: &[u8]) -> Result<Vec<u8>, WgError> {
    fn process_response(&self, pkt: &[u8]) -> Result<Vec<u8>, WgError> {
    fn process_cookie_reply(&self, pkt: &[u8]) -> Result<Vec<u8>, WgError> {
    fn process_transport(
    ) -> Result<Vec<u8>, WgError> {
    pub fn send_keepalive(&self, peer: &WgPeer) -> Result<(), WgError> {
fn rand_u32() -> u32 {
fn generate_x25519_private() -> X25519PrivateKey {
fn derive_handshake_transport_keys(
) -> ([u8; 32], [u8; 32]) {
fn derive_transport_key(
) -> [u8; 32] {
pub struct WgManager {
impl WgManager {
    pub const fn new() -> Self {
    pub fn create_device(&self, name: &str) -> Arc<WgDevice> {
    pub fn delete_device(&self, name: &str) {
    pub fn get_device(&self, name: &str) -> Option<Arc<WgDevice>> {
pub struct WgRuntimeStatus {
pub fn runtime_status() -> WgRuntimeStatus {
pub enum WgError {
pub fn init() {
```

### Matematiksel model

\[nonce_{new}>nonce_{last}\]

\[Replay\;Risk\to 0\;\text{with monotonic window}\]

\[Key\;Rotation\;Interval=\arg\min(C_{cpu}+R_{security})\]

### Muhendislik yorumu

Bu alt sistemde kod parcasi, state gecislerinin hangi sira ve hangi kosullarla ilerledigini gosteren bir publication haritasi gibi okunur. Denklem seti ise bu haritanin tail-latency, dogruluk ve kaynak maliyeti tarafindaki etkisini nicel olarak ifade eder.

Modelde en kritik nokta, tek bir metriği optimize etmek yerine, p99 gecikme, hata yuzeyi ve kaynak tuketimi arasindaki gerilimi ayni anda gormektir. Bu nedenle kararlar satir-bazli kod referansi ile desteklenmeden kabul edilmez.

---

## KM19 - HPACK Huffman decode

Kaynak dosya: `src/net/http2_huffman.rs`

### Kod parcasi

```rust
pub enum HuffmanDecodeError {
enum HuffmanCodeSymbol {
impl HuffmanCodeSymbol {
    fn new(symbol: usize) -> Self {
struct HuffmanDecoder {
impl HuffmanDecoder {
    fn from_table(table: &[(u32, u8)]) -> Self {
    fn new() -> Self {
    fn decode(&mut self, buf: &[u8]) -> Result<Vec<u8>, HuffmanDecodeError> {
struct BitIterator<'a, I: Iterator<Item = &'a u8>> {
impl<'a, I: Iterator<Item = &'a u8>> BitIterator<'a, I> {
    fn new(iterator: I) -> Self {
impl<'a, I: Iterator<Item = &'a u8>> Iterator for BitIterator<'a, I> {
    fn next(&mut self) -> Option<Self::Item> {
pub fn decode_huffman(buf: &[u8]) -> Result<Vec<u8>, HuffmanDecodeError> {
```

### Matematiksel model

\[T_{decode}=O(n_{bits})\]

\[P_{invalid}=P(padding\_bad)+P(eos\_bad)\]

\[FailClosed=1\iff valid\_tree\land valid\_end\]

### Muhendislik yorumu

Bu alt sistemde kod parcasi, state gecislerinin hangi sira ve hangi kosullarla ilerledigini gosteren bir publication haritasi gibi okunur. Denklem seti ise bu haritanin tail-latency, dogruluk ve kaynak maliyeti tarafindaki etkisini nicel olarak ifade eder.

Modelde en kritik nokta, tek bir metriği optimize etmek yerine, p99 gecikme, hata yuzeyi ve kaynak tuketimi arasindaki gerilimi ayni anda gormektir. Bu nedenle kararlar satir-bazli kod referansi ile desteklenmeden kabul edilmez.

---
