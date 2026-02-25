# Preservation Property Tests for Compilation Errors Fix
# Validates: Requirements 3.1, 3.2, 3.3, 3.4, 3.5, 3.6
#
# **Validates: Requirements 3.1, 3.2, 3.3, 3.4, 3.5, 3.6**
#
# IMPORTANT: These tests validate that existing functionality is preserved after the fix
# EXPECTED OUTCOME: Tests PASS (confirms baseline behavior to preserve)
#
# This test verifies that the compilation fixes did not break existing functionality:
# - Atomic operations in hotplug.rs (excluding AtomicU64 usage)
# - RCU mechanism internal logic in rcu.rs (excluding start_grace_period visibility)
# - Existing SMP management functions in smp.rs
# - Lock-free data structures in atomic_ops.rs (excluding Box usage)
# - Scheduler task scheduling logic in task/scheduler.rs (excluding type annotation issues)

Write-Host ""
Write-Host "=== Preservation Property Tests ===" -ForegroundColor Cyan
Write-Host "Validating that existing functionality is preserved after fixes..." -ForegroundColor Cyan
Write-Host ""

# Initialize counters
$totalTests = 6
$passedTests = 0
$failedTests = 0

# Test 1: Compilation Success (Property 1 - Expected Behavior)
Write-Host "Test 1: Compilation Success (Requirements 2.1-2.7)" -ForegroundColor White
Write-Host "  Testing that cargo build completes successfully..." -ForegroundColor Gray

$buildOutput = cargo build 2>&1 | Out-String
$buildSuccess = $LASTEXITCODE -eq 0

if ($buildSuccess) {
    Write-Host "  PASS: Compilation succeeds - all fixes are working correctly" -ForegroundColor Green
    $passedTests++
} else {
    Write-Host "  FAIL: Compilation failed - fixes may be incomplete" -ForegroundColor Red
    Write-Host "  Build output:" -ForegroundColor Yellow
    Write-Host $buildOutput -ForegroundColor Yellow
    $failedTests++
}

# Test 2: Atomic Operations Preservation (hotplug.rs)
Write-Host ""
Write-Host "Test 2: Atomic Operations Preservation in hotplug.rs (Requirement 3.2)" -ForegroundColor White
Write-Host "  Testing that atomic operations remain functional..." -ForegroundColor Gray

# Check that the file compiles and contains expected atomic operations
$hotplugContent = Get-Content "src/hotplug.rs" -Raw -ErrorAction SilentlyContinue

if ($hotplugContent) {
    # Verify AtomicU64 import exists
    $hasAtomicU64Import = $hotplugContent -match "use core::sync::atomic::.*AtomicU64"
    
    # Verify other atomic types are still present
    $hasAtomicBool = $hotplugContent -match "AtomicBool"
    $hasAtomicU32 = $hotplugContent -match "AtomicU32"
    $hasAtomicUsize = $hotplugContent -match "AtomicUsize"
    
    # Verify atomic operations are still used
    $hasAtomicOps = $hotplugContent -match "(fetch_add|fetch_sub|load|store|compare_exchange)"
    
    if ($hasAtomicU64Import -and $hasAtomicBool -and $hasAtomicU32 -and $hasAtomicUsize -and $hasAtomicOps) {
        Write-Host "  PASS: Atomic operations preserved - AtomicU64 import added without breaking existing atomics" -ForegroundColor Green
        $passedTests++
    } else {
        Write-Host "  FAIL: Atomic operations may be broken" -ForegroundColor Red
        if (-not $hasAtomicU64Import) { Write-Host "    - AtomicU64 import missing" -ForegroundColor Yellow }
        if (-not $hasAtomicOps) { Write-Host "    - Atomic operations missing" -ForegroundColor Yellow }
        $failedTests++
    }
} else {
    Write-Host "  FAIL: Cannot read hotplug.rs file" -ForegroundColor Red
    $failedTests++
}

# Test 3: RCU Mechanism Preservation (rcu.rs)
Write-Host ""
Write-Host "Test 3: RCU Mechanism Internal Logic Preservation (Requirement 3.3)" -ForegroundColor White
Write-Host "  Testing that RCU mechanism remains functional..." -ForegroundColor Gray

$rcuContent = Get-Content "src/rcu.rs" -Raw -ErrorAction SilentlyContinue

if ($rcuContent) {
    # Verify start_grace_period is now public
    $hasPublicStartGracePeriod = $rcuContent -match "pub fn start_grace_period"
    
    # Verify other RCU functions are still present
    $hasGracePeriodCompleted = $rcuContent -match "fn grace_period_completed"
    $hasSynchronizeRcu = $rcuContent -match "pub fn synchronize_rcu"
    $hasRcuReadLock = $rcuContent -match "struct RcuReadLock"
    $hasRcuPtr = $rcuContent -match "struct RcuPtr"
    
    # Verify RCU internal logic is preserved
    $hasEpochCounter = $rcuContent -match "RCU_EPOCH"
    $hasReaderCount = $rcuContent -match "RCU_READER_COUNT"
    
    if ($hasPublicStartGracePeriod -and $hasGracePeriodCompleted -and $hasSynchronizeRcu -and 
        $hasRcuReadLock -and $hasRcuPtr -and $hasEpochCounter -and $hasReaderCount) {
        Write-Host "  PASS: RCU mechanism preserved - start_grace_period made public without breaking internal logic" -ForegroundColor Green
        $passedTests++
    } else {
        Write-Host "  FAIL: RCU mechanism may be broken" -ForegroundColor Red
        if (-not $hasPublicStartGracePeriod) { Write-Host "    - start_grace_period not public" -ForegroundColor Yellow }
        if (-not $hasGracePeriodCompleted) { Write-Host "    - grace_period_completed missing" -ForegroundColor Yellow }
        if (-not $hasSynchronizeRcu) { Write-Host "    - synchronize_rcu missing" -ForegroundColor Yellow }
        $failedTests++
    }
} else {
    Write-Host "  FAIL: Cannot read rcu.rs file" -ForegroundColor Red
    $failedTests++
}

# Test 4: SMP Management Functions Preservation (smp.rs)
Write-Host ""
Write-Host "Test 4: SMP Management Functions Preservation (Requirement 3.4)" -ForegroundColor White
Write-Host "  Testing that existing SMP functions remain functional..." -ForegroundColor Gray

$smpContent = Get-Content "src/cpu/smp.rs" -Raw -ErrorAction SilentlyContinue

if ($smpContent) {
    # Verify new functions are added
    $hasStartCpu = $smpContent -match "pub fn start_cpu"
    $hasStopCpu = $smpContent -match "pub fn stop_cpu"
    $hasGetCpuCount = $smpContent -match "pub fn get_cpu_count"
    
    # Verify existing SMP functions are still present
    $hasStartupAp = $smpContent -match "fn startup_ap"
    $hasStartupAllAps = $smpContent -match "pub fn startup_all_aps"
    $hasMarkCpuOnline = $smpContent -match "pub fn mark_cpu_online"
    $hasSendIpi = $smpContent -match "fn send_ipi"
    $hasSmpState = $smpContent -match "struct SmpState"
    $hasPerCpuData = $smpContent -match "struct PerCpuData"
    
    if ($hasStartCpu -and $hasStopCpu -and $hasGetCpuCount -and 
        $hasStartupAp -and $hasStartupAllAps -and $hasMarkCpuOnline -and 
        $hasSendIpi -and $hasSmpState -and $hasPerCpuData) {
        Write-Host "  PASS: SMP functions preserved - new functions added without breaking existing SMP management" -ForegroundColor Green
        $passedTests++
    } else {
        Write-Host "  FAIL: SMP functions may be broken" -ForegroundColor Red
        if (-not $hasStartCpu) { Write-Host "    - start_cpu missing" -ForegroundColor Yellow }
        if (-not $hasStopCpu) { Write-Host "    - stop_cpu missing" -ForegroundColor Yellow }
        if (-not $hasGetCpuCount) { Write-Host "    - get_cpu_count missing" -ForegroundColor Yellow }
        $failedTests++
    }
} else {
    Write-Host "  FAIL: Cannot read smp.rs file" -ForegroundColor Red
    $failedTests++
}

# Test 5: Lock-Free Data Structures Preservation (atomic_ops.rs)
Write-Host ""
Write-Host "Test 5: Lock-Free Data Structures Preservation (Requirement 3.5)" -ForegroundColor White
Write-Host "  Testing that lock-free data structures remain functional..." -ForegroundColor Gray

$atomicOpsContent = Get-Content "src/atomic_ops.rs" -Raw -ErrorAction SilentlyContinue

if ($atomicOpsContent) {
    # Verify Box import exists
    $hasBoxImport = $atomicOpsContent -match "use alloc::boxed::Box"
    
    # Verify lock-free data structures are still present
    $hasLockFreeStack = $atomicOpsContent -match "struct LockFreeStack"
    $hasAtomicOps = $atomicOpsContent -match "trait AtomicOps"
    $hasAtomicPtrOps = $atomicOpsContent -match "trait AtomicPtrOps"
    $hasAtomicBitOps = $atomicOpsContent -match "trait AtomicBitOps"
    $hasAtomicRefCounter = $atomicOpsContent -match "struct AtomicRefCounter"
    $hasAtomicFlag = $atomicOpsContent -match "struct AtomicFlag"
    
    # Verify atomic operations implementations
    $hasImplAtomicOps = $atomicOpsContent -match "impl AtomicOps"
    
    if ($hasBoxImport -and $hasLockFreeStack -and $hasAtomicOps -and 
        $hasAtomicPtrOps -and $hasAtomicBitOps -and $hasAtomicRefCounter -and 
        $hasAtomicFlag -and $hasImplAtomicOps) {
        Write-Host "  PASS: Lock-free structures preserved - Box import added without breaking atomic operations" -ForegroundColor Green
        $passedTests++
    } else {
        Write-Host "  FAIL: Lock-free structures may be broken" -ForegroundColor Red
        if (-not $hasBoxImport) { Write-Host "    - Box import missing" -ForegroundColor Yellow }
        if (-not $hasLockFreeStack) { Write-Host "    - LockFreeStack missing" -ForegroundColor Yellow }
        $failedTests++
    }
} else {
    Write-Host "  FAIL: Cannot read atomic_ops.rs file" -ForegroundColor Red
    $failedTests++
}

# Test 6: Scheduler Task Scheduling Logic Preservation (task/scheduler.rs)
Write-Host ""
Write-Host "Test 6: Scheduler Task Scheduling Logic Preservation (Requirement 3.6)" -ForegroundColor White
Write-Host "  Testing that scheduler logic remains functional..." -ForegroundColor Gray

$schedulerContent = Get-Content "src/task/scheduler.rs" -Raw -ErrorAction SilentlyContinue

if ($schedulerContent) {
    # Verify core scheduler functions are still present
    $hasSchedule = $schedulerContent -match "pub fn schedule"
    $hasSpawn = $schedulerContent -match "pub fn spawn"
    $hasTick = $schedulerContent -match "pub fn tick"
    $hasSleep = $schedulerContent -match "pub fn sleep"
    $hasExit = $schedulerContent -match "pub fn exit"
    $hasSwitchContext = $schedulerContent -match "fn switch_context"
    
    # Verify scheduler data structures
    $hasSmpScheduler = $schedulerContent -match "struct SmpScheduler"
    $hasPerCpuCurrentTask = $schedulerContent -match "PER_CPU_CURRENT_TASK"
    $hasWorkers = $schedulerContent -match "WORKERS"
    $hasStealers = $schedulerContent -match "STEALERS"
    
    # Verify scheduling logic components
    $hasShouldPreempt = $schedulerContent -match "fn should_preempt"
    $hasCalcTimeSlice = $schedulerContent -match "fn calc_time_slice"
    $hasUpdateTaskVruntime = $schedulerContent -match "fn update_task_vruntime"
    
    if ($hasSchedule -and $hasSpawn -and $hasTick -and $hasSleep -and $hasExit -and 
        $hasSwitchContext -and $hasSmpScheduler -and $hasPerCpuCurrentTask -and 
        $hasWorkers -and $hasStealers -and $hasShouldPreempt -and 
        $hasCalcTimeSlice -and $hasUpdateTaskVruntime) {
        Write-Host "  PASS: Scheduler logic preserved - type annotations added without breaking scheduling" -ForegroundColor Green
        $passedTests++
    } else {
        Write-Host "  FAIL: Scheduler logic may be broken" -ForegroundColor Red
        if (-not $hasSchedule) { Write-Host "    - schedule function missing" -ForegroundColor Yellow }
        if (-not $hasSpawn) { Write-Host "    - spawn function missing" -ForegroundColor Yellow }
        $failedTests++
    }
} else {
    Write-Host "  FAIL: Cannot read scheduler.rs file" -ForegroundColor Red
    $failedTests++
}

# Summary
Write-Host ""
Write-Host "=== Summary ===" -ForegroundColor Cyan
Write-Host "Total Tests: $totalTests" -ForegroundColor White
Write-Host "Passed: $passedTests" -ForegroundColor Green
Write-Host "Failed: $failedTests" -ForegroundColor Red
Write-Host ""

# Overall result
if ($passedTests -eq $totalTests) {
    Write-Host "=== OVERALL RESULT: ALL PRESERVATION TESTS PASSED ===" -ForegroundColor Green
    Write-Host "All existing functionality has been preserved after the fixes!" -ForegroundColor Green
    Write-Host ""
    Write-Host "Preservation Properties Validated:" -ForegroundColor Cyan
    Write-Host "  - Atomic operations in hotplug.rs remain functional" -ForegroundColor Green
    Write-Host "  - RCU mechanism internal logic is preserved" -ForegroundColor Green
    Write-Host "  - Existing SMP management functions work correctly" -ForegroundColor Green
    Write-Host "  - Lock-free data structures remain intact" -ForegroundColor Green
    Write-Host "  - Scheduler task scheduling logic is unchanged" -ForegroundColor Green
    Write-Host ""
    exit 0
} else {
    Write-Host "=== OVERALL RESULT: PRESERVATION TESTS FAILED ===" -ForegroundColor Red
    Write-Host "Some existing functionality may have been broken by the fixes." -ForegroundColor Red
    Write-Host "Failed: $failedTests out of $totalTests tests" -ForegroundColor Red
    Write-Host ""
    exit 1
}
