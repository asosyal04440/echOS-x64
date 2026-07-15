import gdb
import struct

"""echOS GDB custom commands for kernel debugging."""


def _read_phys_addr(addr, size, cache=None):
    """Read physical memory via the kernel's identity mapping or HHDM."""
    if cache is not None:
        hhdm_offset = cache.get('hhdm_offset')
    else:
        hhdm_offset = None
    if hhdm_offset is None:
        try:
            hhdm_offset = int(gdb.parse_and_eval('physical_memory_offset'))
        except Exception:
            try:
                hhdm_offset = int(gdb.parse_and_eval('KERNEL_HHDM_OFFSET'))
            except Exception:
                try:
                    hhdm_offset = int(gdb.parse_and_eval('HHDM_OFFSET'))
                except Exception:
                    hhdm_offset = None
    if hhdm_offset is not None:
        virt_addr = addr + hhdm_offset
        try:
            ptr = gdb.Value(virt_addr).cast(gdb.lookup_type('void').pointer())
            inferior = gdb.selected_inferior()
            return inferior.read_memory(ptr, size)
        except Exception:
            pass
    try:
        ptr = gdb.Value(addr).cast(gdb.lookup_type('void').pointer())
        inferior = gdb.selected_inferior()
        return inferior.read_memory(ptr, size)
    except Exception:
        return None


def _get_current_cr3():
    """Read CR3 register from the current CPU."""
    try:
        cr3_str = gdb.execute('p/x $cr3', to_string=True)
        for token in cr3_str.split():
            if token.startswith('0x') or token.startswith('$'):
                val = token.replace('$', '').replace(',', '')
                try:
                    return int(val, 16)
                except ValueError:
                    continue
    except Exception:
        pass
    return None


def _page_table_entry_name(paddr, level):
    """Return a human-readable description for a page table entry."""
    if paddr == 0:
        return "Not Present"
    present = bool(paddr & 1)
    if not present:
        return "Not Present"
    writable = bool(paddr & (1 << 1))
    user = bool(paddr & (1 << 2))
    accessed = bool(paddr & (1 << 5))
    dirty = bool(paddr & (1 << 6))
    huge = bool(paddr & (1 << 7)) if level <= 2 else False
    nx = bool(paddr & (1 << 63))
    phys = paddr & 0x000FFFFFFFFFF000
    attrs = []
    if writable:
        attrs.append('W')
    if user:
        attrs.append('U')
    if accessed:
        attrs.append('A')
    if dirty:
        attrs.append('D')
    if nx:
        attrs.append('NX')
    if huge:
        attrs.append('H')
    attr_str = '|'.join(attrs) if attrs else '-'
    return "{:#018x} [{}] -> {:#018x}".format(paddr, attr_str, phys)


class KernelLog(gdb.Command):
    """Display kernel log ring buffer contents.

Usage: kernel-log [count]
  count  Number of recent entries to show (default: 50)
    """

    def __init__(self):
        super(KernelLog, self).__init__("kernel-log", gdb.COMMAND_USER)

    def _find_log_buffer(self):
        """Try to locate the kernel log buffer by symbol name."""
        symbols = ['KERNEL_LOG', 'kernel_log_buffer', 'LOG_BUF',
                    'KERNEL_LOG_BUF', 'log_buf', 'kernel_log']
        for sym in symbols:
            try:
                val = gdb.parse_and_eval(sym)
                if val is not None:
                    return val, sym
            except Exception:
                continue
        return None, None

    def invoke(self, arg, from_tty):
        args = arg.strip().split()
        count = 50
        if args:
            try:
                count = int(args[0])
            except ValueError:
                print("Usage: kernel-log [count]")
                return
        buf_val, sym_name = self._find_log_buffer()
        if buf_val is not None:
            print("Kernel log buffer found at symbol '{}'.".format(sym_name))
            try:
                for i in range(min(count, 1000)):
                    try:
                        entry = buf_val[i]
                        entry_str = str(entry)
                        if entry_str and entry_str.strip() not in ('', '0', '\\0'):
                            print("[{}] {}".format(i, entry_str))
                    except Exception:
                        break
            except Exception as e:
                print("Error reading log buffer: {}".format(e))
        else:
            print("No kernel log buffer symbol found.")
            print("Fallback: reading serial output buffer from IO port 0x3F8...")
            try:
                out = gdb.execute('p/x (*(volatile unsigned char*)0x3F8)', to_string=True)
                print("UART status: {}".format(out.strip()))
            except Exception:
                print("Cannot read UART port directly in GDB.")
                print("Try: 'x/{}bx symbol_name' to dump a known log buffer.".format(count))


class KernelBacktrace(gdb.Command):
    """Enhanced backtrace with kernel-specific information.

Usage: kernel-backtrace [task]
  task  Optional pointer to a Task struct for remote backtrace
    """

    def __init__(self):
        super(KernelBacktrace, self).__init__("kernel-backtrace", gdb.COMMAND_USER)

    def _print_backtrace(self, rbp, rip, rsp):
        """Walk the RBP-linked stack frames."""
        print("Backtrace (RBP-chain walk):")
        frame_num = 0
        depth = 0
        max_depth = 64
        current_rbp = rbp
        current_rip = rip
        inferior = gdb.selected_inferior()
        print("  #{}  {:#018x} (suspected)".format(frame_num, current_rip))
        frame_num += 1
        while depth < max_depth:
            try:
                if current_rbp == 0:
                    break
                rbp_mem = inferior.read_memory(current_rbp, 16)
                vals = struct.unpack('<QQ', rbp_mem)
                next_rbp = vals[0]
                ret_addr = vals[1]
                if ret_addr == 0 or next_rbp == current_rbp:
                    break
                print("  #{}  {:#018x}".format(frame_num, ret_addr))
                current_rbp = next_rbp
                depth += 1
                frame_num += 1
            except Exception:
                break

    def invoke(self, arg, from_tty):
        args = arg.strip().split()
        if args:
            task_ptr_str = args[0]
            try:
                task_ptr = gdb.parse_and_eval(task_ptr_str)
                if task_ptr is not None:
                    try:
                        task_deref = task_ptr.dereference()
                        context = task_deref['context']
                        rbp = int(context['rbp'])
                        rip = int(context['rip'])
                        rsp = int(context['rsp'])
                        print("Task backtrace (from Task context):")
                        self._print_backtrace(rbp, rip, rsp)
                        return
                    except Exception as e:
                        print("Failed to read Task context: {}".format(e))
            except Exception as e:
                print("Invalid task expression: {}".format(e))
                return
        try:
            rbp = int(gdb.parse_and_eval('$rbp'))
            rip = int(gdb.parse_and_eval('$rip'))
            rsp = int(gdb.parse_and_eval('$rsp'))
        except Exception:
            print("Cannot read current frame registers. Are you in a live debug session?")
            return
        self._print_backtrace(rbp, rip, rsp)


class KernelRegs(gdb.Command):
    """Display kernel registers with annotations.

Usage: kernel-regs
    """

    def __init__(self):
        super(KernelRegs, self).__init__("kernel-regs", gdb.COMMAND_USER)

    def _reg(self, name):
        try:
            val = int(gdb.parse_and_eval('${}'.format(name)))
            return val
        except Exception:
            return None

    def _annotate_cr0(self, val):
        if val is None:
            return ""
        bits = []
        if val & (1 << 0):
            bits.append('PE')
        if val & (1 << 1):
            bits.append('MP')
        if val & (1 << 2):
            bits.append('EM')
        if val & (1 << 3):
            bits.append('TS')
        if val & (1 << 4):
            bits.append('ET')
        if val & (1 << 5):
            bits.append('NE')
        if val & (1 << 16):
            bits.append('WP')
        if val & (1 << 18):
            bits.append('AM')
        if val & (1 << 29):
            bits.append('NW')
        if val & (1 << 30):
            bits.append('CD')
        if val & (1 << 31):
            bits.append('PG')
        return " | " + " ".join(bits)

    def _annotate_rflags(self, val):
        if val is None:
            return ""
        bits = []
        if val & (1 << 0):
            bits.append('CF')
        if val & (1 << 2):
            bits.append('PF')
        if val & (1 << 4):
            bits.append('AF')
        if val & (1 << 6):
            bits.append('ZF')
        if val & (1 << 7):
            bits.append('SF')
        if val & (1 << 8):
            bits.append('TF')
        if val & (1 << 9):
            bits.append('IF')
        if val & (1 << 10):
            bits.append('DF')
        if val & (1 << 11):
            bits.append('OF')
        if val & (1 << 12):
            bits.append('IOPL')
        if val & (1 << 14):
            bits.append('NT')
        if val & (1 << 16):
            bits.append('RF')
        if val & (1 << 17):
            bits.append('VM')
        if val & (1 << 18):
            bits.append('AC')
        if val & (1 << 19):
            bits.append('VIF')
        if val & (1 << 20):
            bits.append('VIP')
        if val & (1 << 21):
            bits.append('ID')
        return " | " + " ".join(bits)

    def invoke(self, arg, from_tty):
        regs = {
            'General Purpose': [
                ('rax', None), ('rbx', None), ('rcx', None), ('rdx', None),
                ('rsi', None), ('rdi', None), ('rbp', None), ('rsp', None),
                ('r8', None), ('r9', None), ('r10', None), ('r11', None),
                ('r12', None), ('r13', None), ('r14', None), ('r15', None),
                ('rip', None), ('rflags', self._annotate_rflags),
            ],
            'Segment': [
                ('cs', None), ('ss', None), ('ds', None), ('es', None),
                ('fs', None), ('gs', None),
            ],
            'Control': [
                ('cr0', self._annotate_cr0), ('cr2', None), ('cr3', None),
                ('cr4', None),
            ],
        }
        print("=== Kernel Registers ===")
        for group_name, reg_list in regs.items():
            print("\n--- {} ---".format(group_name))
            for name, annotator in reg_list:
                val = self._reg(name)
                if val is not None:
                    annotation = annotator(val) if annotator else ""
                    print("  {:6s} = {:#018x}{}".format(name, val, annotation))
                else:
                    print("  {:6s} = <unavailable>".format(name))


class DumpPageTables(gdb.Command):
    """Walk and display kernel page tables.

Usage: dump-page-tables [cr3_value]
  cr3_value  Optional physical address of PML4 (default: read CR3)
    """

    def __init__(self):
        super(DumpPageTables, self).__init__("dump-page-tables", gdb.COMMAND_USER)

    def _read_qword(self, addr, cache):
        """Read a 64-bit value from a physical address."""
        data = _read_phys_addr(addr, 8, cache)
        if data is None:
            return None
        return struct.unpack('<Q', data)[0]

    def _walk_page_table(self, pml4_phys, cache, verbose=True):
        """Walk the x86_64 4-level page table hierarchy."""
        if pml4_phys is None or pml4_phys == 0:
            print("Invalid PML4 address.")
            return
        pml4_phys = pml4_phys & 0x000FFFFFFFFFF000
        print("Walking page table from PML4 = {:#018x}".format(pml4_phys))
        if verbose:
            print("Format: [addr] flags -> physical\n")
        total_entries = 0
        mapped_regions = 0
        for pml4_idx in range(512):
            pml4e_addr = pml4_phys + pml4_idx * 8
            pml4e = self._read_qword(pml4e_addr, cache)
            if pml4e is None:
                break
            if pml4e & 1:
                total_entries += 1
                pdpt_phys = pml4e & 0x000FFFFFFFFFF000
                for pdpt_idx in range(512):
                    pdpte_addr = pdpt_phys + pdpt_idx * 8
                    pdpte = self._read_qword(pdpte_addr, cache)
                    if pdpte is None:
                        break
                    if pdpte & 1:
                        total_entries += 1
                        if pdpte & (1 << 7):
                            if verbose:
                                vaddr = (pml4_idx << 39) | (pdpt_idx << 30)
                                sign = vaddr if vaddr < (1 << 47) else vaddr - (1 << 64)
                                print("  1G Page: {:#018x} {}".format(sign, _page_table_entry_name(pdpte, 1)))
                            mapped_regions += 1
                            continue
                        pd_phys = pdpte & 0x000FFFFFFFFFF000
                        for pd_idx in range(512):
                            pde_addr = pd_phys + pd_idx * 8
                            pde = self._read_qword(pde_addr, cache)
                            if pde is None:
                                break
                            if pde & 1:
                                total_entries += 1
                                if pde & (1 << 7):
                                    if verbose:
                                        vaddr = (pml4_idx << 39) | (pdpt_idx << 30) | (pd_idx << 21)
                                        sign = vaddr if vaddr < (1 << 47) else vaddr - (1 << 64)
                                        print("  2M Page: {:#018x} {}".format(sign, _page_table_entry_name(pde, 2)))
                                    mapped_regions += 1
                                    continue
                                pt_phys = pde & 0x000FFFFFFFFFF000
                                for pt_idx in range(512):
                                    pte_addr = pt_phys + pt_idx * 8
                                    pte = self._read_qword(pte_addr, cache)
                                    if pte is None:
                                        break
                                    if pte & 1:
                                        total_entries += 1
                                        if verbose:
                                            vaddr = (pml4_idx << 39) | (pdpt_idx << 30) | (pd_idx << 21) | (pt_idx << 12)
                                            sign = vaddr if vaddr < (1 << 47) else vaddr - (1 << 64)
                                            print("    4K Page: {:#018x} {}".format(sign, _page_table_entry_name(pte, 3)))
                                        mapped_regions += 1
        print("\nSummary: {} total entries, {} mapped regions".format(total_entries, mapped_regions))

    def invoke(self, arg, from_tty):
        args = arg.strip().split()
        cache = {}
        try:
            cache['hhdm_offset'] = int(gdb.parse_and_eval('physical_memory_offset'))
        except Exception:
            pass
        if args:
            try:
                cr3_val = int(args[0], 0)
            except ValueError:
                print("Invalid CR3 value.")
                return
        else:
            cr3_val = _get_current_cr3()
        if cr3_val is None or cr3_val == 0:
            print("Cannot determine CR3 value. Specify a PML4 physical address.")
            return
        self._walk_page_table(cr3_val, cache)


class KernelInfo(gdb.Command):
    """Display echOS kernel version and build info.

Usage: kernel-info
    """

    def __init__(self):
        super(KernelInfo, self).__init__("kernel-info", gdb.COMMAND_USER)

    def invoke(self, arg, from_tty):
        print("=== echOS Kernel Info ===")
        symbols = [
            ('echOS_VERSION', 'Kernel version'),
            ('KERNEL_VERSION', 'Kernel version'),
            ('KERNEL_NAME', 'Kernel name'),
            ('BUILD_TIME', 'Build time'),
            ('BUILD_DATE', 'Build date'),
            ('KERNEL_BUILD_TIME', 'Build time'),
            ('GIT_HASH', 'Git hash'),
            ('BUILD_PROFILE', 'Build profile'),
        ]
        for sym, label in symbols:
            try:
                val = gdb.parse_and_eval(sym)
                val_str = str(val)
                print("  {}: {}".format(label, val_str.strip('"')))
            except Exception:
                pass
        try:
            cr3 = _get_current_cr3()
            if cr3:
                print("  Active PML4: {:#018x}".format(cr3))
        except Exception:
            pass
        try:
            cpu_vendor = gdb.parse_and_eval('$cpucmd')
            print("  CPU: {}".format(cpu_vendor))
        except Exception:
            pass


def register_commands():
    """Register all echOS GDB commands."""
    KernelLog()
    KernelBacktrace()
    KernelRegs()
    DumpPageTables()
    KernelInfo()
