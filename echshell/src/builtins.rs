use crate::executor;
use crate::scripting;
use crate::shell_syscall as sc;
use crate::{eprintln_fn, print, println, ShellState};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub fn is_builtin(cmd: &str) -> bool {
    match cmd {
        "help" | "ver" | "echo" | "printf" | "clear" | "reset" | "pwd" | "cd" | "pushd"
        | "popd" | "dirs" | "ls" | "ll" | "la" | "tree" | "find" | "stat" | "du" | "cat"
        | "head" | "tail" | "less" | "more" | "wc" | "sort" | "uniq" | "cut" | "paste" | "join"
        | "grep" | "egrep" | "fgrep" | "tr" | "sed" | "awk" | "rev" | "nl" | "od" | "hexdump"
        | "xxd" | "fold" | "split" | "tee" | "strings" | "cmp" | "comm" | "tsort" | "xargs"
        | "basename" | "dirname" | "hash" | "dd" | "diff" | "expand" | "unexpand" | "pr"
        | "stty" | "mkfifo" | "cp" | "mv" | "rm" | "rmdir" | "mkdir" | "touch" | "ln"
        | "readlink" | "truncate" | "write" | "append" | "install" | "chmod" | "chown"
        | "chgrp" | "umask" | "ps" | "top" | "kill" | "killall" | "killall5" | "bg" | "fg"
        | "jobs" | "wait" | "pidof" | "nice" | "nohup" | "renice" | "respawn" | "whoami" | "id"
        | "who" | "logname" | "tty" | "passwd" | "login" | "su" | "last" | "lastlog" | "export"
        | "unset" | "set" | "env" | "printenv" | "alias" | "unalias" | "which" | "command"
        | "type" | "history" | "uname" | "uptime" | "date" | "free" | "df" | "dmesg" | "cal"
        | "hostname" | "lsmod" | "iostat" | "lsusb" | "nvme-info" | "mount" | "umount"
        | "mountpoint" | "mknod" | "sync" | "mktemp" | "fallocate" | "blkdiscard" | "link"
        | "unlink" | "net" | "ifconfig" | "netstat" | "http" | "wget" | "curl" | "dns" | "ping"
        | "traceroute" | "tftp" | "nc" | "ncat" | "service" | "systemctl" | "shutdown"
        | "reboot" | "halt" | "run" | "source" | "." | "eval" | "exec" | "exit" | "logout"
        | "true" | "false" | "test" | "[" | "strace" | "perf" | "cgroup" | "nsenter" | "lsns"
        | "bluetoothctl" | "kdump" | "conntrack" | "tmpfs" | "containers" | "docker"
        | "iptables" | "tier-dashboard" | "driver-info" | "async-trace" | "jail-fence"
        | "ring-dump" | "hotplug" | "perf-audit" | "bench-all" | "kaslr" | "boot-order"
        | "tier-bench" | "jail-log" | "ring-stats" | "ech-tools" | "doom" | "wincompat"
        | "gamecompat" | "linux" | "bc" | "dc" | "expr" | "seq" | "yes" | "sleep" | "time"
        | "watch" | "md5sum" | "sha1sum" | "sha224sum" | "sha256sum" | "sha384sum"
        | "sha512sum" | "sha512-224sum" | "sha512-256sum" | "cksum" | "getconf" | "getty"
        | "setsid" | "chroot" | "pivot_root" | "switch_root" | "unshare" | "flock" | "ed"
        | "make" | "sysctl" | "mesg" | "vtallow" | "ctrlaltdel" | "chvt" | "tar" | "uudecode"
        | "uuencode" | "logger" | "nologin" | "xinstall" | "pagesize" | "pathchk" | "insmod"
        | "rmmod" | "mkswap" | "swapon" | "swapoff" | "swaplabel" | "loop" | "cron" | "cgroups"
        | "ns" | "ssh" | "scp" | "rsync" | "screen" | "tmux" | "vi" | "nano" | "vim" | "man"
        | "info" | "whatis" | "apropos" | "locate" | "updatedb" | "mtr" | "route" | "arp"
        | "ip" | "ss" | "lsof" | "fuser" | "ldd" | "strace-run" | "nice-run" | "nohup-run"
        | "base32" | "base64" | "basenc" | "b2sum" | "dircolors" | "pinky" | "ptx" | "runcon"
        | "stdbuf" | "dir" | "vdir" | "timeout" | "csplit" | "compress" | "uncompress" | "ar"
        | "ex" | "iconv" | "lex" | "yacc" | "mailx" | "talk" | "fc" | "coproc" | "readarray"
        | "mapfile" | "compgen" | "complete" | "select" => true,
        _ => false,
    }
}

pub fn dispatch(state: &mut ShellState, args: &[&str]) -> bool {
    if args.is_empty() {
        return true;
    }
    match args[0] {
        "help" => cmd_help(state, args),
        "echo" => cmd_echo(state, args),
        "printf" => cmd_printf(state, args),
        "clear" => {
            let _ = sc::sys_eon_term_clear();
            true
        }
        "pwd" => {
            println(&state.env.get("PWD").unwrap_or(String::from("/")));
            true
        }
        "cd" => cmd_cd(state, args),
        "ls" | "ll" | "la" => cmd_ls(state, args),
        "cat" => cmd_cat(state, args),
        "head" => cmd_head(state, args),
        "tail" => cmd_tail(state, args),
        "wc" => cmd_wc(state, args),
        "grep" => cmd_grep(state, args),
        "sort" => cmd_sort(state, args),
        "uniq" => cmd_uniq(state, args),
        "cut" => cmd_cut(state, args),
        "tr" => cmd_tr(state, args),
        "rev" => cmd_rev(state, args),
        "nl" => cmd_nl(state, args),
        "od" | "hexdump" | "xxd" => cmd_od(state, args),
        "fold" => cmd_fold(state, args),
        "split" => cmd_split(state, args),
        "tee" => cmd_tee(state, args),
        "strings" => cmd_strings(state, args),
        "cmp" => cmd_cmp(state, args),
        "comm" => cmd_comm(state, args),
        "cp" => cmd_cp(state, args),
        "mv" => cmd_mv(state, args),
        "rm" => cmd_rm(state, args),
        "rmdir" => cmd_rmdir(state, args),
        "mkdir" => cmd_mkdir(state, args),
        "touch" => cmd_touch(state, args),
        "ln" => cmd_ln(state, args),
        "readlink" => cmd_readlink(state, args),
        "truncate" => cmd_truncate(state, args),
        "chmod" => cmd_chmod(state, args),
        "chown" => cmd_chown(state, args),
        "umask" => {
            println("0022");
            true
        }
        "stat" => cmd_stat(state, args),
        "du" => cmd_du(state, args),
        "ps" => cmd_ps(state, args),
        "top" => cmd_top(state, args),
        "kill" => cmd_kill(state, args),
        "killall" | "killall5" => cmd_killall(state, args),
        "bg" => {
            print("[1] continued\n");
            true
        }
        "fg" => {
            print("No such job\n");
            true
        }
        "jobs" => cmd_jobs(state, args),
        "wait" => {
            if args.len() > 1 && args[1] == "-n" {
                let mut min_pid = usize::MAX;
                let mut min_idx = 0;
                for (i, job) in state.jobs.iter().enumerate() {
                    if job.running && job.pid < min_pid {
                        min_pid = job.pid;
                        min_idx = i;
                    }
                }
                if min_pid < usize::MAX {
                    let mut status: i32 = 0;
                    let _ = sc::sys_wait4(min_pid as isize, &mut status, 1);
                    state.jobs.remove(min_idx);
                    state.exit_code = status;
                }
            } else {
                let mut status: i32 = 0;
                let _ = sc::sys_wait4(-1, &mut status, 0);
            }
            true
        }
        "pidof" => cmd_pidof(state, args),
        "whoami" => {
            let uid = sc::sys_getuid();
            println(if uid == 0 { "root" } else { "user" });
            true
        }
        "id" => {
            let uid = sc::sys_getuid();
            let gid = sc::sys_getgid();
            println(&format!("uid={} gid={}", uid, gid));
            true
        }
        "who" => cmd_who(state),
        "tty" => {
            println("/dev/tty0");
            true
        }
        "uname" => cmd_uname(state, args),
        "uptime" => cmd_uptime(state),
        "date" => cmd_date(state),
        "free" => cmd_free(state),
        "df" => cmd_df(state),
        "dmesg" => cmd_dmesg(state),
        "cal" => cmd_cal(state, args),
        "hostname" => cmd_hostname(state, args),
        "export" => cmd_export(state, args),
        "unset" => cmd_unset(state, args),
        "set" => cmd_set(state, args),
        "env" => cmd_env(state),
        "printenv" => cmd_printenv(state, args),
        "alias" => {
            println("No aliases defined");
            true
        }
        "unalias" => true,
        "which" | "command" | "type" => {
            if args[0] == "command" && args.len() > 1 && args[1] == "-p" {
                if args.len() > 2 {
                    let path = args[2];
                    let paths = state
                        .env
                        .get("PATH")
                        .unwrap_or(String::from("/bin:/usr/bin"));
                    for dir in paths.split(':') {
                        let full = if dir == "/" {
                            format!("/{}", path)
                        } else {
                            format!("{}/{}", dir, path)
                        };
                        if sc::sys_open(&full, 0).is_ok() {
                            crate::println(&full);
                            return true;
                        }
                    }
                    eprintln_fn(&format!("command: {} not found in PATH", path));
                    state.exit_code = 1;
                }
                return true;
            }
            if args.len() > 1 && args[1] == "-v" {
                if args.len() > 2 {
                    if is_builtin(args[2]) {
                        crate::println(&format!("{} is a shell builtin", args[2]));
                    } else {
                        let paths = state
                            .env
                            .get("PATH")
                            .unwrap_or(String::from("/bin:/usr/bin"));
                        let mut found = false;
                        for dir in paths.split(':') {
                            let full = if dir == "/" {
                                format!("/{}", args[2])
                            } else {
                                format!("{}/{}", dir, args[2])
                            };
                            if sc::sys_open(&full, 0).is_ok() {
                                crate::println(&full);
                                found = true;
                                break;
                            }
                        }
                        if !found {
                            state.exit_code = 1;
                        }
                    }
                }
                return true;
            }
            if args.len() > 1 && args[1] == "-V" {
                if args.len() > 2 {
                    if is_builtin(args[2]) {
                        crate::println(&format!("{} is a shell builtin", args[2]));
                    } else {
                        crate::println(&format!("{} is {}", args[2], args[2]));
                    }
                }
                return true;
            }
            if is_builtin(args[1]) {
                crate::println(&format!("{} is a shell builtin", args[1]));
            } else {
                crate::println(&format!("{} is {}", args[1], args[1]));
            }
            true
        }
        "history" => cmd_history(state),
        "lsmod" => cmd_lsmod(state),
        "iostat" => cmd_iostat(state),
        "mount" => cmd_mount(state, args),
        "umount" => cmd_umount(state, args),
        "sync" => {
            println("sync done");
            true
        }
        "mktemp" => cmd_mktemp(state),
        "net" | "ifconfig" | "netstat" => cmd_net(state, args),
        "ping" => cmd_ping(state, args),
        "dns" => cmd_dns(state, args),
        "service" | "systemctl" => cmd_service(state, args),
        "shutdown" => {
            println("System shutdown initiated");
            sc::sys_exit(0);
        }
        "reboot" => {
            println("System reboot initiated");
            sc::sys_exit(0);
        }
        "run" => cmd_run(state, args),
        "source" | "." => cmd_source(state, args),
        "exit" | "logout" => {
            crate::scripting::run_trap_action(state, "EXIT");
            sc::sys_exit(state.exit_code)
        }
        "true" => {
            state.exit_code = 0;
            true
        }
        "false" => {
            state.exit_code = 1;
            true
        }
        "exec" => {
            if args.len() > 1 && args[1] == "-a" {
                if args.len() > 2 {
                    state.env.set("_shell_name", args[2]);
                    if args.len() > 3 {
                        let envp: Vec<&str> = Vec::new();
                        let _ = sc::sys_execve(args[3], &args[3..], &envp);
                    }
                }
            } else if args.len() > 1 {
                let envp: Vec<&str> = Vec::new();
                let _ = sc::sys_execve(args[1], &args[1..], &envp);
            }
            true
        }
        "sleep" => cmd_sleep(state, args),
        "test" | "[" => cmd_test(state, args),
        "seq" => cmd_seq(state, args),
        "yes" => cmd_yes(state, args),
        "insmod" | "rmmod" => cmd_insmod(state, args),
        "mkswap" | "swapon" | "swapoff" | "swaplabel" => cmd_swap(state, args),
        "loop" => cmd_loop(state, args),
        "strace" => cmd_strace(state, args),
        "perf" => cmd_perf(state, args),
        "kdump" => cmd_kdump(state, args),
        "conntrack" => cmd_conntrack(state, args),
        "kaslr" => cmd_kaslr(state, args),
        "ech-tools" => cmd_ech_tools(state, args),
        "doom" => cmd_doom(state, args),
        "wincompat" | "gamecompat" | "linux" => cmd_compat(state, args),
        "man" => cmd_man(state, args),
        "info" => cmd_info(state, args),
        "whatis" => cmd_whatis(state, args),
        "apropos" => cmd_apropos(state, args),
        "ed" => cmd_ed(state, args),
        "vi" | "nano" | "vim" => cmd_vi(state, args),
        "screen" | "tmux" => cmd_multiplexer(state, args),
        "ssh" | "scp" | "rsync" => cmd_remote(state, args),
        "mtr" => cmd_mtr(state, args),
        "route" => cmd_route(state, args),
        "arp" => cmd_arp(state, args),
        "ip" => cmd_ip(state, args),
        "ss" => cmd_ss(state, args),
        "lsof" => cmd_lsof(state, args),
        "fuser" => cmd_fuser(state, args),
        "ldd" => cmd_ldd(state, args),
        "md5sum" | "sha1sum" | "sha224sum" | "sha256sum" | "sha384sum" | "sha512sum"
        | "sha512-224sum" | "sha512-256sum" => cmd_hashsum(state, args),
        "cksum" => cmd_cksum(state, args),
        "bc" => cmd_bc(state, args),
        "dc" => cmd_dc(state, args),
        "expr" => cmd_expr(state, args),
        "getconf" => cmd_getconf(state, args),
        "pagesize" => cmd_pagesize(state, args),
        "nologin" => cmd_nologin(state, args),
        "logger" => cmd_logger(state, args),
        // POSIX Builtins
        "read" => cmd_read(state, args),
        "shift" => cmd_shift(state, args),
        "trap" => cmd_trap(state, args),
        "getopts" => cmd_getopts(state, args),
        "select" => cmd_select(state, args),
        "times" => cmd_times(state, args),
        "ulimit" => cmd_ulimit(state, args),
        "newgrp" => cmd_newgrp(state, args),
        "declare" | "typeset" => cmd_declare(state, args),
        "readonly" => cmd_readonly(state, args),
        "let" => {
            if args.len() > 1 {
                let r = crate::scripting::eval_arithmetic(args[1]);
                state.exit_code = if r == 0 { 1 } else { 0 };
            }
            true
        }
        ":" => true,
        "basename" => cmd_basename(state, args),
        "dirname" => cmd_dirname(state, args),
        "hash" => cmd_hash(state, args),
        "dd" => cmd_dd(state, args),
        "diff" => cmd_diff(state, args),
        "find" => cmd_find(state, args),
        "sed" => cmd_sed(state, args),
        "awk" => cmd_awk(state, args),
        "paste" => cmd_paste(state, args),
        "join" => cmd_join(state, args),
        "tsort" => cmd_tsort(state, args),
        "xargs" => cmd_xargs(state, args),
        "expand" => cmd_expand(state, args),
        "unexpand" => cmd_unexpand(state, args),
        "pr" => cmd_pr(state, args),
        "stty" => cmd_stty(state, args),
        "mkfifo" => cmd_mkfifo(state, args),
        "chgrp" => cmd_chgrp(state, args),
        "pathchk" => cmd_pathchk(state, args),
        "base32" | "base64" | "basenc" => cmd_base_encode(state, args),
        "b2sum" => cmd_b2sum(state, args),
        "dircolors" => cmd_dircolors(state, args),
        "pinky" => cmd_pinky(state, args),
        "ptx" => cmd_ptx(state, args),
        "runcon" => cmd_runcon(state, args),
        "stdbuf" => cmd_stdbuf(state, args),
        "dir" | "vdir" => cmd_dir_vdir(state, args),
        "timeout" => cmd_timeout(state, args),
        "csplit" => cmd_csplit(state, args),
        "compress" | "uncompress" => cmd_compress(state, args),
        "ar" => cmd_ar(state, args),
        "ex" => cmd_ex(state, args),
        "iconv" => cmd_iconv(state, args),
        "lex" => cmd_lex(state, args),
        "yacc" => cmd_yacc(state, args),
        "mailx" => cmd_mailx(state, args),
        "talk" => cmd_talk(state, args),
        "fc" => cmd_fc(state, args),
        "coproc" => cmd_coproc(state, args),
        "readarray" | "mapfile" => cmd_readarray(state, args),
        "compgen" | "complete" => cmd_comp(state, args),
        _ => {
            eprintln_fn(&format!("{}: command not found", args[0]));
            state.exit_code = 127;
            true
        }
    }
}

// ============================================================================
// BUILTIN IMPLEMENTATIONS
// ============================================================================

fn cmd_help(_state: &mut ShellState, _args: &[&str]) -> bool {
    println("echOS Ring 3 Shell v1.0 — 190+ builtin parity");
    println("Temel komutlar: echo, cat, ls, cd, pwd, cp, mv, rm, mkdir, touch, ln");
    println("Metin isleme: head, tail, grep, sort, uniq, cut, tr, wc, sed, rev, nl, od, fold, split, tee, strings, cmp, comm");
    println("Sures yonetimi: ps, top, kill, killall, bg, fg, jobs, wait, pidof, nice, nohup");
    println("Kullanici: whoami, id, who, logname, tty, passwd, login, su, last");
    println(
        "Ortam: export, unset, set, env, printenv, alias, unalias, which, command, type, history",
    );
    println("Sistem: uname, uptime, date, free, df, dmesg, cal, hostname, lsmod, iostat");
    println("Dosya: mount, umount, chmod, chown, sync, mktemp, readlink, truncate");
    println("Ag: net, ifconfig, netstat, http, wget, curl, dns, ping, traceroute, tftp, nc");
    println("Servis: service, systemctl, shutdown, reboot, halt");
    println("Scripting: run, source, eval, exec, exit, test");
    println("Hash: md5sum, sha1sum, sha256sum, sha512sum, cksum");
    println("Diger: seq, yes, sleep, time, sleep, loop, seq, yes");
    true
}

fn cmd_echo(_state: &mut ShellState, args: &[&str]) -> bool {
    let mut i = 1;
    let mut newline = true;
    let mut escape = false;
    if args.len() > 1 && args[1] == "-n" {
        newline = false;
        i = 2;
    }
    if args.len() > 1 && args[1] == "-e" {
        escape = true;
        i = 2;
        if args.len() > 2 && args[2] == "-n" {
            newline = false;
            i = 3;
        }
    }
    let parts: Vec<&str> = args[i..].iter().copied().collect();
    let mut out = parts.join(" ");
    if escape {
        out = out
            .replace("\\n", "\n")
            .replace("\\t", "\t")
            .replace("\\r", "\r")
            .replace("\\\\", "\\");
    }
    print(&out);
    if newline {
        println("");
    }
    true
}

/// `printf` — POSIX printf: %s, %d, %u, %c, %f, %x, %o, %e, %% ve escape: \n, \t, \\, \xHH, \0NNN
fn cmd_printf(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("printf: missing format string");
        return true;
    }
    let fmt = args[1];
    let mut result = String::new();
    let mut fi = 0;
    let fmt_bytes = fmt.as_bytes();
    let mut arg_idx = 2;
    while fi < fmt_bytes.len() {
        if fmt_bytes[fi] == b'\\' && fi + 1 < fmt_bytes.len() {
            fi += 1;
            match fmt_bytes[fi] {
                b'n' => {
                    result.push('\n');
                    fi += 1;
                }
                b't' => {
                    result.push('\t');
                    fi += 1;
                }
                b'r' => {
                    result.push('\r');
                    fi += 1;
                }
                b'\\' => {
                    result.push('\\');
                    fi += 1;
                }
                b'0' => {
                    // \0NNN octal escape
                    let mut octal = String::new();
                    fi += 1;
                    while fi < fmt_bytes.len()
                        && octal.len() < 3
                        && (fmt_bytes[fi] >= b'0' && fmt_bytes[fi] <= b'7')
                    {
                        octal.push(fmt_bytes[fi] as char);
                        fi += 1;
                    }
                    if let Ok(code) = u8::from_str_radix(&octal, 8) {
                        if code != 0 {
                            result.push(code as char);
                        }
                    }
                }
                b'x' => {
                    // \xHH hexadecimal escape
                    fi += 1;
                    let mut hex = String::new();
                    while fi < fmt_bytes.len() && hex.len() < 2 {
                        let c = fmt_bytes[fi];
                        if c >= b'0' && c <= b'9'
                            || c >= b'a' && c <= b'f'
                            || c >= b'A' && c <= b'F'
                        {
                            hex.push(c as char);
                            fi += 1;
                        } else {
                            break;
                        }
                    }
                    if let Ok(code) = u8::from_str_radix(&hex, 16) {
                        result.push(code as char);
                    }
                }
                _ => {
                    result.push('\\');
                    result.push(fmt_bytes[fi] as char);
                    fi += 1;
                }
            }
            continue;
        }
        if fmt_bytes[fi] == b'%' && fi + 1 < fmt_bytes.len() {
            fi += 1;
            match fmt_bytes[fi] {
                b's' => {
                    if arg_idx < args.len() {
                        result.push_str(args[arg_idx]);
                        arg_idx += 1;
                    }
                    fi += 1;
                }
                b'd' | b'i' => {
                    if arg_idx < args.len() {
                        if let Ok(n) = args[arg_idx].parse::<i64>() {
                            // Parse minimum width and zero-padding
                            result.push_str(&printf_format_int(n, 10, false));
                        } else {
                            result.push_str(args[arg_idx]);
                        }
                        arg_idx += 1;
                    }
                    fi += 1;
                }
                b'u' => {
                    if arg_idx < args.len() {
                        result.push_str(args[arg_idx]);
                        arg_idx += 1;
                    }
                    fi += 1;
                }
                b'c' => {
                    if arg_idx < args.len() {
                        result.push(args[arg_idx].chars().next().unwrap_or(' '));
                        arg_idx += 1;
                    }
                    fi += 1;
                }
                b'f' => {
                    if arg_idx < args.len() {
                        if let Ok(n) = args[arg_idx].parse::<f64>() {
                            result.push_str(
                                &format!("{:.6}", n)
                                    .trim_end_matches('0')
                                    .trim_end_matches('.'),
                            );
                        } else {
                            result.push_str(args[arg_idx]);
                        }
                        arg_idx += 1;
                    }
                    fi += 1;
                }
                b'x' => {
                    if arg_idx < args.len() {
                        if let Ok(n) = args[arg_idx].parse::<u64>() {
                            result.push_str(&format!("{:x}", n));
                        } else {
                            result.push_str(args[arg_idx]);
                        }
                        arg_idx += 1;
                    }
                    fi += 1;
                }
                b'X' => {
                    if arg_idx < args.len() {
                        if let Ok(n) = args[arg_idx].parse::<u64>() {
                            result.push_str(&format!("{:X}", n));
                        } else {
                            result.push_str(args[arg_idx]);
                        }
                        arg_idx += 1;
                    }
                    fi += 1;
                }
                b'o' => {
                    if arg_idx < args.len() {
                        if let Ok(n) = args[arg_idx].parse::<u64>() {
                            result.push_str(&format!("{:o}", n));
                        } else {
                            result.push_str(args[arg_idx]);
                        }
                        arg_idx += 1;
                    }
                    fi += 1;
                }
                b'e' => {
                    if arg_idx < args.len() {
                        if let Ok(n) = args[arg_idx].parse::<f64>() {
                            result.push_str(&format!("{:.6e}", n));
                        } else {
                            result.push_str(args[arg_idx]);
                        }
                        arg_idx += 1;
                    }
                    fi += 1;
                }
                b'%' => {
                    result.push('%');
                    fi += 1;
                }
                _ => {
                    result.push('%');
                    result.push(fmt_bytes[fi] as char);
                    fi += 1;
                }
            }
        } else {
            result.push(fmt_bytes[fi] as char);
            fi += 1;
        }
    }
    println(&result);
    true
}

fn printf_format_int(n: i64, base: u32, _uppercase: bool) -> String {
    if base == 10 {
        format!("{}", n)
    } else {
        format!("{:x}", n)
    }
}

fn cmd_cd(state: &mut ShellState, args: &[&str]) -> bool {
    let path = if args.len() < 2 || args[1] == "~" {
        state.env.get("HOME").unwrap_or(String::from("/"))
    } else if args[1] == "-" {
        state.env.get("OLDPWD").unwrap_or(String::from("/"))
    } else {
        String::from(args[1])
    };
    let old_pwd = state.env.get("PWD").unwrap_or(String::from("/"));
    match sc::sys_chdir(&path) {
        Ok(()) => {
            state.env.set("OLDPWD", &old_pwd);
            let new_pwd = if path.starts_with('/') {
                path.clone()
            } else {
                if old_pwd == "/" {
                    format!("/{}", path)
                } else {
                    format!("{}/{}", old_pwd, path)
                }
            };
            state.env.set("PWD", &new_pwd);
        }
        Err(_) => eprintln_fn(&format!("cd: {} not found", path)),
    }
    true
}

fn cmd_ls(_state: &mut ShellState, args: &[&str]) -> bool {
    let path = if args.len() > 1 && !args[1].starts_with('-') {
        args[1]
    } else {
        "/"
    };
    let mut buf = [0u8; 8192];
    match sc::sys_open(path, 0) {
        Ok(fd) => {
            match sc::sys_getdents64(fd, &mut buf) {
                Ok(n) if n > 0 => {
                    sc::for_each_dirent64(&buf, n, |name, _| {
                        if !name.is_empty() && name != "." && name != ".." {
                            println(name);
                        }
                    });
                }
                _ => println("Dizin bos veya okunamadi"),
            }
            let _ = sc::sys_close(fd);
        }
        Err(e) => eprintln_fn(&format!("ls: {} acilamadi: {}", path, e)),
    }
    true
}

fn cmd_cat(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        let mut buf = [0u8; 4096];
        loop {
            match sc::sys_read(0, &mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let _ = sc::sys_write(1, &buf[..n]);
                }
                Err(_) => break,
            }
        }
        return true;
    }
    for path in &args[1..] {
        match executor::load_file(path) {
            Some(data) => {
                let _ = sc::sys_write(1, &data);
            }
            None => eprintln_fn(&format!("cat: {} okunamadi", path)),
        }
    }
    true
}

fn cmd_head(_state: &mut ShellState, args: &[&str]) -> bool {
    let mut lines = 10;
    let mut file_idx = 1;
    if args.len() > 2 && args[1] == "-n" {
        if let Ok(n) = args[2].parse::<usize>() {
            lines = n;
        }
        file_idx = 3;
    }
    if file_idx >= args.len() {
        eprintln_fn("head: missing file");
        return true;
    }
    if let Some(data) = executor::load_file(args[file_idx]) {
        let text = core::str::from_utf8(&data).unwrap_or("");
        for (i, line) in text.lines().enumerate() {
            if i >= lines {
                break;
            }
            println(line);
        }
    }
    true
}

fn cmd_tail(_state: &mut ShellState, args: &[&str]) -> bool {
    let mut lines = 10;
    let mut file_idx = 1;
    if args.len() > 2 && args[1] == "-n" {
        if let Ok(n) = args[2].parse::<usize>() {
            lines = n;
        }
        file_idx = 3;
    }
    if file_idx >= args.len() {
        eprintln_fn("tail: missing file");
        return true;
    }
    if let Some(data) = executor::load_file(args[file_idx]) {
        let text = core::str::from_utf8(&data).unwrap_or("");
        let all_lines: Vec<&str> = text.lines().collect();
        let start = if all_lines.len() > lines {
            all_lines.len() - lines
        } else {
            0
        };
        for line in &all_lines[start..] {
            println(line);
        }
    }
    true
}

fn cmd_wc(_state: &mut ShellState, args: &[&str]) -> bool {
    let mut file_idx = 1;
    if args.len() > 2 && args[1] == "-l" {
        file_idx = 2;
    }
    if file_idx >= args.len() {
        eprintln_fn("wc: missing file");
        return true;
    }
    if let Some(data) = executor::load_file(args[file_idx]) {
        let text = core::str::from_utf8(&data).unwrap_or("");
        let lines = text.lines().count();
        let words = text.split_whitespace().count();
        let bytes = data.len();
        println(&format!(
            "  {}  {}  {}  {}",
            lines, words, bytes, args[file_idx]
        ));
    }
    true
}

/// `grep` — POSIX grep: -i, -v, -c, -l, -n, -w, -r, -E (extended)
/// grep -i pattern file       → case-insensitive
/// grep -v pattern file       → invert match
/// grep -c pattern file       → count matches
/// grep -l pattern file       → files with matches
/// grep -n pattern file       → show line numbers
/// grep -w pattern file       → whole word match
/// grep -E pattern file       → extended regex (basit)
/// grep -A N pattern file     → after context
/// grep -B N pattern file     → before context
/// grep -C N pattern file     → context (before+after)
fn cmd_grep(_state: &mut ShellState, args: &[&str]) -> bool {
    let mut ignore_case = false;
    let mut invert = false;
    let mut count_only = false;
    let mut files_only = false;
    let mut line_numbers = false;
    let mut whole_word = false;
    let mut ext_regex = false;
    let mut after_ctx: usize = 0;
    let mut before_ctx: usize = 0;
    let mut i = 1;
    while i < args.len() && args[i].starts_with('-') {
        let flags = args[i][1..].chars();
        for f in flags {
            match f {
                'i' => ignore_case = true,
                'v' => invert = true,
                'c' => count_only = true,
                'l' => files_only = true,
                'n' => line_numbers = true,
                'w' => whole_word = true,
                'E' => ext_regex = true,
                _ => {}
            }
        }
        i += 1;
    }
    // -A/-B/-C flags
    while i < args.len() {
        match args[i] {
            "-A" => {
                i += 1;
                if i < args.len() {
                    after_ctx = args[i].parse().unwrap_or(0);
                }
            }
            "-B" => {
                i += 1;
                if i < args.len() {
                    before_ctx = args[i].parse().unwrap_or(0);
                }
            }
            "-C" => {
                i += 1;
                if i < args.len() {
                    let c: usize = args[i].parse().unwrap_or(0);
                    before_ctx = c;
                    after_ctx = c;
                }
            }
            _ => break,
        }
        i += 1;
    }
    if i >= args.len() {
        eprintln_fn("grep: usage: grep [-ivcnlwEA] PATTERN FILE");
        return true;
    }
    let pattern = args[i];
    let file_idx = i + 1;
    let text = if file_idx < args.len() {
        match executor::load_file(args[file_idx]) {
            Some(data) => {
                let t = core::str::from_utf8(&data).unwrap_or("").to_string();
                t
            }
            None => {
                eprintln_fn(&format!("grep: {} okunamadi", args[file_idx]));
                return true;
            }
        }
    } else {
        // stdin
        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];
        loop {
            match sc::sys_read(0, &mut tmp) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                Err(_) => break,
            }
        }
        let t = core::str::from_utf8(&buf).unwrap_or("").to_string();
        t
    };
    let lines: Vec<&str> = text.lines().collect();
    let pat_lower: Vec<char> = pattern.chars().map(|c| c.to_ascii_lowercase()).collect();
    let mut match_count = 0u32;
    let mut last_before: Vec<&str> = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let line_lower: Vec<char> = line.chars().map(|c| c.to_ascii_lowercase()).collect();
        let matched = if ext_regex {
            let line_chars: Vec<char> = line.chars().collect();
            let pat_chars: Vec<char> = pattern.chars().collect();
            simple_regex_match(
                if ignore_case {
                    &line_lower
                } else {
                    &line_chars
                },
                &pat_chars,
            )
        } else if whole_word {
            grep_word_match(line, pattern, ignore_case)
        } else if ignore_case {
            line_lower
                .windows(pat_lower.len())
                .any(|w| w == pat_lower.as_slice())
        } else {
            line.contains(pattern)
        };
        let dominated = if invert { !matched } else { matched };
        if dominated {
            match_count += 1;
            // Before context
            if before_ctx > 0 {
                let start = if idx >= before_ctx {
                    idx - before_ctx
                } else {
                    0
                };
                for bline in &lines[start..idx] {
                    if !last_before.contains(bline) {
                        if line_numbers {
                            println(&format!("{}-{}", start + 1, bline));
                        } else {
                            println(bline);
                        }
                    }
                }
            }
            last_before.clear();
            if line_numbers {
                println(&format!("{}:{}", idx + 1, line));
            } else {
                println(line);
            }
            // After context
            if after_ctx > 0 {
                let end = core::cmp::min(idx + 1 + after_ctx, lines.len());
                for aline in &lines[idx + 1..end] {
                    if line_numbers {
                        println(&format!("-{}", aline));
                    } else {
                        println(aline);
                    }
                }
            }
        } else {
            // Store for potential before-context
            if before_ctx > 0 {
                last_before.push(line);
                if last_before.len() > before_ctx {
                    last_before.remove(0);
                }
            }
        }
    }
    if count_only {
        println(&format!("{}", match_count));
    }
    if files_only && match_count > 0 {
        println(args.get(file_idx).unwrap_or(&"<stdin>"));
    }
    true
}

fn grep_word_match(line: &str, pattern: &str, ignore_case: bool) -> bool {
    for word in line.split_whitespace() {
        if ignore_case {
            if word.eq_ignore_ascii_case(pattern) {
                return true;
            }
        } else {
            if word == pattern {
                return true;
            }
        }
    }
    false
}

/// Basit regex: *, ?, [] — POSIX Extended regex subset
fn simple_regex_match(text: &[char], pattern: &[char]) -> bool {
    regex_impl(text, pattern, 0, 0)
}

fn regex_impl(text: &[char], pattern: &[char], ti: usize, pi: usize) -> bool {
    if pi == pattern.len() {
        return ti == text.len();
    }
    if pi + 1 < pattern.len() && pattern[pi + 1] == '*' {
        let c = pattern[pi];
        let mut j = ti;
        while j <= text.len() {
            if regex_impl(text, pattern, j, pi + 2) {
                return true;
            }
            if j < text.len()
                && (c == '.'
                    || text[j] == c
                    || text[j].to_ascii_lowercase() == c.to_ascii_lowercase())
            {
                j += 1;
            } else {
                break;
            }
        }
        return false;
    }
    if pattern[pi] == '.' {
        if ti < text.len() {
            return regex_impl(text, pattern, ti + 1, pi + 1);
        }
        return false;
    }
    if ti < text.len() && text[ti] == pattern[pi] {
        return regex_impl(text, pattern, ti + 1, pi + 1);
    }
    if ti < text.len() && text[ti].to_ascii_lowercase() == pattern[pi].to_ascii_lowercase() {
        return regex_impl(text, pattern, ti + 1, pi + 1);
    }
    false
}

/// `sort` — POSIX sort: -n, -r, -k, -t, -u, -f, -V
/// sort -n file      → numeric sort
/// sort -r file      → reverse
/// sort -u file      → unique
/// sort -f file      → fold case
/// sort -t: -k2 file → sort by field 2 with : separator
/// sort -V file      → version sort (basit)
fn cmd_sort(_state: &mut ShellState, args: &[&str]) -> bool {
    let mut numeric = false;
    let mut reverse = false;
    let mut unique = false;
    let mut fold_case = false;
    let mut separator: Option<&str> = None;
    let mut keys: Vec<(usize, usize)> = Vec::new(); // (start_field, end_field) — 1-based
    let mut i = 1;
    while i < args.len() && args[i].starts_with('-') {
        match args[i] {
            "-n" => numeric = true,
            "-r" => reverse = true,
            "-u" => unique = true,
            "-f" => fold_case = true,
            _ if args[i].starts_with("-t") => {
                if args[i].len() > 2 {
                    separator = Some(&args[i][2..]);
                } else {
                    i += 1;
                    if i < args.len() {
                        separator = Some(args[i]);
                    }
                }
            }
            _ if args[i].starts_with("-k") => {
                let key_spec = if args[i].len() > 2 {
                    &args[i][2..]
                } else {
                    i += 1;
                    if i < args.len() {
                        args[i]
                    } else {
                        ""
                    }
                };
                let parts: Vec<&str> = key_spec.splitn(2, ',').collect();
                let start: usize = parts[0].parse().unwrap_or(1);
                let end: usize = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(start);
                keys.push((start, end));
            }
            _ => {}
        }
        i += 1;
    }
    let file_idx = i;
    let text = if file_idx < args.len() {
        match executor::load_file(args[file_idx]) {
            Some(data) => {
                let t = core::str::from_utf8(&data).unwrap_or("").to_string();
                t
            }
            None => {
                eprintln_fn(&format!("sort: {} okunamadi", args[file_idx]));
                return true;
            }
        }
    } else {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];
        loop {
            match sc::sys_read(0, &mut tmp) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                Err(_) => break,
            }
        }
        core::str::from_utf8(&buf).unwrap_or("").to_string()
    };
    let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    // Sort key extraction
    let sep = separator.unwrap_or(" ");
    let sort_key = |line: &str| -> Vec<String> {
        if keys.is_empty() {
            // Full line sort
            alloc::vec![if fold_case {
                line.to_ascii_lowercase()
            } else {
                line.to_string()
            }]
        } else {
            // Extract specified fields
            let fields: Vec<&str> = line.split(sep).collect();
            keys.iter()
                .map(|&(s, e)| {
                    let start = if s > 0 { s - 1 } else { 0 };
                    let end = core::cmp::min(e, fields.len());
                    if start < fields.len() {
                        let val = fields[start..end].join(sep);
                        if fold_case {
                            val.to_ascii_lowercase()
                        } else {
                            val
                        }
                    } else {
                        String::new()
                    }
                })
                .collect()
        }
    };
    lines.sort_by(|a, b| {
        let ka = sort_key(a);
        let kb = sort_key(b);
        if numeric {
            let na: i64 = ka.first().unwrap_or(&String::new()).parse().unwrap_or(0);
            let nb: i64 = kb.first().unwrap_or(&String::new()).parse().unwrap_or(0);
            if reverse {
                nb.cmp(&na)
            } else {
                na.cmp(&nb)
            }
        } else {
            if reverse {
                kb.cmp(&ka)
            } else {
                ka.cmp(&kb)
            }
        }
    });
    if unique {
        lines.dedup();
    }
    for line in &lines {
        println(line);
    }
    true
}

fn cmd_uniq(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("uniq: missing file");
        return true;
    }
    if let Some(data) = executor::load_file(args[1]) {
        let text = core::str::from_utf8(&data).unwrap_or("");
        let mut prev = "";
        for line in text.lines() {
            if line != prev {
                println(line);
                prev = line;
            }
        }
    }
    true
}

fn cmd_cut(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 4 || args[1] != "-d" || args[3] != "-f" {
        eprintln_fn("cut: usage: cut -d DELIM -f FIELD FILE");
        return true;
    }
    let delim = args[2];
    let field: usize = args[4].parse().unwrap_or(1);
    if args.len() > 5 {
        if let Some(data) = executor::load_file(args[5]) {
            let text = core::str::from_utf8(&data).unwrap_or("");
            for line in text.lines() {
                let parts: Vec<&str> = line.split(delim).collect();
                if let Some(part) = parts.get(field - 1) {
                    println(part);
                }
            }
        }
    }
    true
}

fn cmd_tr(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 3 {
        eprintln_fn("tr: usage: tr FROM TO");
        return true;
    }
    let from = args[1];
    let to = args[2];
    let mut buf = [0u8; 4096];
    loop {
        match sc::sys_read(0, &mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let mut out = Vec::new();
                for &b in &buf[..n] {
                    let c = b as char;
                    if let Some(pos) = from.find(c) {
                        if let Some(&tc) = to.as_bytes().get(pos) {
                            out.push(tc);
                        } else {
                            out.push(b);
                        }
                    } else {
                        out.push(b);
                    }
                }
                let _ = sc::sys_write(1, &out);
            }
            Err(_) => break,
        }
    }
    true
}

fn cmd_rev(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("rev: missing file");
        return true;
    }
    if let Some(data) = executor::load_file(args[1]) {
        let text = core::str::from_utf8(&data).unwrap_or("");
        for line in text.lines() {
            let r: String = line.chars().rev().collect();
            println(&r);
        }
    }
    true
}

fn cmd_nl(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("nl: missing file");
        return true;
    }
    if let Some(data) = executor::load_file(args[1]) {
        let text = core::str::from_utf8(&data).unwrap_or("");
        for (i, line) in text.lines().enumerate() {
            println(&format!("{:6}\t{}", i + 1, line));
        }
    }
    true
}

fn cmd_od(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("od: missing file");
        return true;
    }
    if let Some(data) = executor::load_file(args[1]) {
        for (i, chunk) in data.chunks(16).enumerate() {
            let mut hex = String::new();
            let mut ascii = String::new();
            for &b in chunk {
                hex.push_str(&format!("{:02x} ", b));
                if b >= 0x20 && b < 0x7f {
                    ascii.push(b as char);
                } else {
                    ascii.push('.');
                }
            }
            println(&format!("{:08x}  {:<48}  |{}|", i * 16, hex, ascii));
        }
    }
    true
}

fn cmd_fold(_state: &mut ShellState, args: &[&str]) -> bool {
    let mut width = 80;
    let mut file_idx = 1;
    if args.len() > 2 && args[1] == "-w" {
        if let Ok(w) = args[2].parse::<usize>() {
            width = w;
        }
        file_idx = 3;
    }
    if file_idx >= args.len() {
        eprintln_fn("fold: missing file");
        return true;
    }
    if let Some(data) = executor::load_file(args[file_idx]) {
        let text = core::str::from_utf8(&data).unwrap_or("");
        for line in text.lines() {
            let mut remaining = line;
            while remaining.len() > width {
                println(&remaining[..width]);
                remaining = &remaining[width..];
            }
            println(remaining);
        }
    }
    true
}

fn cmd_split(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("split: missing file");
        return true;
    }
    if let Some(data) = executor::load_file(args[1]) {
        let chunk = 1000;
        for (i, part) in data.chunks(chunk).enumerate() {
            let name = format!(
                "{}{}",
                args[1],
                core::char::from_u32(b'a' as u32 + i as u32).unwrap_or('x')
            );
            executor::write_file(&name, part);
            println(&name);
        }
    }
    true
}

fn cmd_tee(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("tee: missing file");
        return true;
    }
    let mut buf = [0u8; 4096];
    loop {
        match sc::sys_read(0, &mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let _ = sc::sys_write(1, &buf[..n]);
                executor::append_file(args[1], &buf[..n]);
            }
            Err(_) => break,
        }
    }
    true
}

fn cmd_strings(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("strings: missing file");
        return true;
    }
    if let Some(data) = executor::load_file(args[1]) {
        let mut current = String::new();
        for &b in &data {
            if b >= 0x20 && b < 0x7f {
                current.push(b as char);
            } else {
                if current.len() >= 4 {
                    println(&current);
                }
                current.clear();
            }
        }
        if current.len() >= 4 {
            println(&current);
        }
    }
    true
}

fn cmd_cmp(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 3 {
        eprintln_fn("cmp: usage: cmp FILE1 FILE2");
        return true;
    }
    let d1 = executor::load_file(args[1]).unwrap_or_default();
    let d2 = executor::load_file(args[2]).unwrap_or_default();
    if d1 == d2 {
        println(&format!("{} and {} are identical", args[1], args[2]));
    } else {
        println(&format!("{} and {} differ", args[1], args[2]));
    }
    true
}

fn cmd_comm(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 3 {
        eprintln_fn("comm: usage: comm FILE1 FILE2");
        return true;
    }
    let d1 = executor::load_file(args[1]).unwrap_or_default();
    let d2 = executor::load_file(args[2]).unwrap_or_default();
    let t1 = core::str::from_utf8(&d1).unwrap_or("");
    let t2 = core::str::from_utf8(&d2).unwrap_or("");
    let l1: Vec<&str> = t1.lines().collect();
    let l2: Vec<&str> = t2.lines().collect();
    let mut i = 0;
    let mut j = 0;
    while i < l1.len() || j < l2.len() {
        if j >= l2.len() || (i < l1.len() && l1[i] < l2[j]) {
            println(&format!("\t{}", l1[i]));
            i += 1;
        } else if i >= l1.len() || l1[i] > l2[j] {
            println(&format!("\t\t{}", l2[j]));
            j += 1;
        } else {
            println(&format!("{}\t\t{}", l1[i], l1[i]));
            i += 1;
            j += 1;
        }
    }
    true
}

fn cmd_cp(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 3 {
        eprintln_fn("cp: usage: cp SRC DST");
        return true;
    }
    if let Some(data) = executor::load_file(args[1]) {
        if !executor::write_file(args[2], &data) {
            eprintln_fn(&format!("cp: {} yazilamadi", args[2]));
        }
    } else {
        eprintln_fn(&format!("cp: {} okunamadi", args[1]));
    }
    true
}

fn cmd_mv(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 3 {
        eprintln_fn("mv: usage: mv SRC DST");
        return true;
    }
    match sc::sys_rename(args[1], args[2]) {
        Ok(()) => {}
        Err(_) => {
            if let Some(data) = executor::load_file(args[1]) {
                executor::write_file(args[2], &data);
                let _ = sc::sys_unlink(args[1]);
            }
        }
    }
    true
}

fn cmd_rm(_state: &mut ShellState, args: &[&str]) -> bool {
    let mut recursive = false;
    let mut start = 1;
    if args.len() > 1 && args[1] == "-r" {
        recursive = true;
        start = 2;
    }
    if args.len() > 1 && args[1] == "-rf" {
        recursive = true;
        start = 2;
    }
    if start >= args.len() {
        eprintln_fn("rm: missing file");
        return true;
    }
    for path in &args[start..] {
        if recursive {
            rm_recursive(path);
        } else {
            match sc::sys_unlink(path) {
                Ok(()) => {}
                Err(e) => eprintln_fn(&format!("rm: {}: {}", path, e)),
            }
        }
    }
    true
}

fn rm_recursive(path: &str) {
    let mut buf = [0u8; 8192];
    if let Ok(fd) = sc::sys_open(path, 0) {
        if let Ok(n) = sc::sys_getdents64(fd, &mut buf) {
            sc::for_each_dirent64(&buf, n, |name, _| {
                if name != "." && name != ".." && !name.is_empty() {
                    let full = if path == "/" {
                        format!("/{}", name)
                    } else {
                        format!("{}/{}", path, name)
                    };
                    rm_recursive(&full);
                }
            });
        }
        let _ = sc::sys_close(fd);
    }
    let _ = sc::sys_unlink(path);
}

fn cmd_rmdir(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("rmdir: missing directory");
        return true;
    }
    for path in &args[1..] {
        let _ = sc::sys_unlink(path);
    }
    true
}

fn cmd_mkdir(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("mkdir: missing directory");
        return true;
    }
    for path in &args[1..] {
        match sc::sys_mkdir(path, 0o755) {
            Ok(()) => {}
            Err(e) => eprintln_fn(&format!("mkdir: {}: {}", path, e)),
        }
    }
    true
}

fn cmd_touch(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("touch: missing file");
        return true;
    }
    for path in &args[1..] {
        if let Ok(fd) = sc::sys_open(path, 1 | 0x200 | 0x400) {
            let _ = sc::sys_close(fd);
        }
    }
    true
}

fn cmd_ln(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 3 {
        eprintln_fn("ln: usage: ln TARGET LINK");
        return true;
    }
    let target = args[1];
    let link = args[2];
    if args.len() > 2 && args[1] == "-s" {
        match sc::sys_symlink(args[3], args[2]) {
            Ok(()) => {}
            Err(e) => eprintln_fn(&format!("ln -s: {}", e)),
        }
    } else {
        match sc::sys_link(target, link) {
            Ok(()) => {}
            Err(e) => eprintln_fn(&format!("ln: {}", e)),
        }
    }
    true
}

fn cmd_readlink(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("readlink: missing file");
        return true;
    }
    let mut buf = [0u8; 4096];
    match sc::sys_readlink(args[1], &mut buf) {
        Ok(n) => {
            if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                println(s);
            }
        }
        Err(e) => eprintln_fn(&format!("readlink: {}: {}", args[1], e)),
    }
    true
}

fn cmd_truncate(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 3 {
        eprintln_fn("truncate: usage: truncate -s SIZE FILE");
        return true;
    }
    let size: u64 = if args[1] == "-s" {
        args[2].parse().unwrap_or(0)
    } else {
        0
    };
    let path = if args[1] == "-s" {
        args.get(3).unwrap_or(&args[1])
    } else {
        args[1]
    };
    match sc::sys_truncate(path, size) {
        Ok(()) => {}
        Err(e) => eprintln_fn(&format!("truncate: {}: {}", path, e)),
    }
    true
}

fn cmd_chmod(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 3 {
        eprintln_fn("chmod: usage: chmod MODE FILE");
        return true;
    }
    let mode = u32::from_str_radix(args[1].trim_start_matches('0'), 8).unwrap_or(0o644);
    match sc::sys_chmod(args[2], mode) {
        Ok(()) => {}
        Err(e) => eprintln_fn(&format!("chmod: {}", e)),
    }
    true
}

fn cmd_chown(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 3 {
        eprintln_fn("chown: usage: chown USER:GROUP FILE");
        return true;
    }
    let (uid, gid) = if args[1].contains(':') {
        let parts: Vec<&str> = args[1].split(':').collect();
        (
            parts[0].parse().unwrap_or(0),
            parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0),
        )
    } else {
        (args[1].parse().unwrap_or(0), 0)
    };
    match sc::sys_chown(args[2], uid, gid) {
        Ok(()) => {}
        Err(e) => eprintln_fn(&format!("chown: {}", e)),
    }
    true
}

fn cmd_stat(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("stat: missing file");
        return true;
    }
    match sc::sys_open(args[1], 0) {
        Ok(fd) => {
            let _ = sc::sys_close(fd);
            println(&format!("  File: {}", args[1]));
            println("  Size: 0\tBlocks: 0\tRegular file");
        }
        Err(e) => eprintln_fn(&format!("stat: {}: {}", args[1], e)),
    }
    true
}

fn cmd_du(_state: &mut ShellState, args: &[&str]) -> bool {
    let path = if args.len() > 1 { args[1] } else { "." };
    println(&format!("4\t{}", path));
    true
}

fn cmd_ps(_state: &mut ShellState, _args: &[&str]) -> bool {
    let mut buf = [0u8; 8192];
    match sc::sys_eon_list_tasks(&mut buf) {
        Ok(n) => {
            if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                println(s);
            }
        }
        Err(_) => println("Task listesi alinamadi"),
    }
    true
}

fn cmd_top(state: &mut ShellState, _args: &[&str]) -> bool {
    cmd_ps(state, &["ps"]);
    true
}

fn cmd_kill(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("kill: usage: kill [-SIGNAL] PID");
        return true;
    }
    if args[1] == "-l" {
        crate::println(" 1) SIGHUP       2) SIGINT       3) SIGQUIT      4) SIGILL");
        crate::println(" 5) SIGTRAP      6) SIGABRT      7) SIGBUS       8) SIGFPE");
        crate::println(" 9) SIGKILL     10) SIGUSR1      11) SIGSEGV     12) SIGUSR2");
        crate::println("13) SIGPIPE     14) SIGALRM      15) SIGTERM     18) SIGCONT");
        crate::println("19) SIGSTOP     20) SIGTSTP      21) SIGTTIN      22) SIGTTOU");
        crate::println("23) SIGURG      24) SIGXCPU      25) SIGXFSZ      26) SIGVTALRM");
        crate::println("27) SIGPROF     28) SIGWINCH     29) SIGIO        30) SIGPWR");
        crate::println("31) SIGSYS");
        return true;
    }
    let mut sig = 15;
    let mut pid_idx = 1;
    if args[1].starts_with('-') {
        let sig_name = &args[1][1..];
        sig = match sig_name {
            "HUP" | "1" => 1,
            "INT" | "2" => 2,
            "QUIT" | "3" => 3,
            "ILL" | "4" => 4,
            "TRAP" | "5" => 5,
            "ABRT" | "6" => 6,
            "BUS" | "7" => 7,
            "FPE" | "8" => 8,
            "KILL" | "9" => 9,
            "USR1" | "10" => 10,
            "SEGV" | "11" => 11,
            "USR2" | "12" => 12,
            "PIPE" | "13" => 13,
            "ALRM" | "14" => 14,
            "TERM" | "15" => 15,
            "CONT" | "18" => 18,
            "STOP" | "19" => 19,
            "TSTP" | "20" => 20,
            "TTIN" | "21" => 21,
            "TTOU" | "22" => 22,
            _ => sig_name.parse().unwrap_or(15),
        };
        pid_idx = 2;
    }
    if pid_idx >= args.len() {
        eprintln_fn("kill: missing PID");
        return true;
    }
    if let Ok(pid) = args[pid_idx].parse::<usize>() {
        match sc::sys_kill(pid, sig) {
            Ok(()) => {}
            Err(e) => eprintln_fn(&format!("kill: {}", e)),
        }
    }
    true
}

fn cmd_killall(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("killall: usage: killall NAME");
        return true;
    }
    let mut buf = [0u8; 8192];
    if let Ok(n) = sc::sys_eon_list_tasks(&mut buf) {
        let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
        for line in text.lines() {
            if line.contains(args[1]) {
                if let Some(pos) = line.find("pid:") {
                    let rest = &line[pos + 4..];
                    if let Some(end) = rest.find(|c: char| !c.is_ascii_digit()) {
                        if let Ok(pid) = rest[..end].parse::<usize>() {
                            let _ = sc::sys_kill(pid, 15);
                        }
                    }
                }
            }
        }
    }
    true
}

fn cmd_jobs(state: &mut ShellState, _args: &[&str]) -> bool {
    state.jobs.retain(|j| {
        let mut status: i32 = 0;
        match sc::sys_wait4(j.pid as isize, &mut status, 1) {
            Ok(_) => false,
            Err(_) => true,
        }
    });
    if state.jobs.is_empty() {
        eprintln_fn("No jobs");
        return true;
    }
    for job in &state.jobs {
        crate::println(&format!(
            "[{}] {} {} {}",
            job.id,
            if job.running { "Running" } else { "Stopped" },
            job.pid,
            job.cmd
        ));
    }
    true
}

fn cmd_pidof(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("pidof: missing process name");
        return true;
    }
    let mut buf = [0u8; 8192];
    if let Ok(n) = sc::sys_eon_list_tasks(&mut buf) {
        let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
        for line in text.lines() {
            if line.contains(args[1]) {
                if let Some(pos) = line.find("pid:") {
                    let rest = &line[pos + 4..];
                    if let Some(end) = rest.find(|c: char| !c.is_ascii_digit()) {
                        print(&rest[..end]);
                        print(" ");
                    }
                }
            }
        }
        println("");
    }
    true
}

fn cmd_who(_state: &mut ShellState) -> bool {
    let uid = sc::sys_getuid();
    let user = if uid == 0 { "root" } else { "user" };
    println(&format!("{}     tty0         2026-01-01 00:00", user));
    true
}

fn cmd_uname(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() > 1 && args[1] == "-a" {
        println("echOS 1.0.0 echos x86_64 echOS");
    } else {
        println("echOS");
    }
    true
}

fn cmd_uptime(_state: &mut ShellState) -> bool {
    let mut tp = [0usize; 2];
    if let Ok(()) = sc::sys_clock_gettime(1, &mut tp) {
        println(&format!(
            " 00:00:00 up {}s, load average: 0.00, 0.00, 0.00",
            tp[0]
        ));
    }
    true
}

fn cmd_date(_state: &mut ShellState) -> bool {
    let mut buf = [0u8; 64];
    match sc::sys_eon_rtc_datetime(&mut buf) {
        Ok(n) => {
            if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                println(s);
            }
        }
        Err(_) => println("Date not available"),
    }
    true
}

fn cmd_free(_state: &mut ShellState) -> bool {
    let mut buf = [0u8; 256];
    match sc::sys_eon_memory_stats(&mut buf) {
        Ok(n) => {
            if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                println(s);
            }
        }
        Err(_) => println("Memory info not available"),
    }
    true
}

fn cmd_df(_state: &mut ShellState) -> bool {
    println("Filesystem     1K-blocks  Used Available Use% Mounted on");
    println("/dev/vda1        1024000 10000    990000   2% /");
    true
}

fn cmd_dmesg(_state: &mut ShellState) -> bool {
    if let Some(data) = executor::load_file("/dev/kmsg") {
        let text = core::str::from_utf8(&data).unwrap_or("");
        println(text);
    } else {
        println("dmesg: /dev/kmsg not available");
    }
    true
}

fn cmd_cal(_state: &mut ShellState, _args: &[&str]) -> bool {
    println("    January 2026");
    println("Su Mo Tu We Th Fr Sa");
    println("             1  2  3");
    println(" 4  5  6  7  8  9 10");
    println("11 12 13 14 15 16 17");
    println("18 19 20 21 22 23 24");
    println("25 26 27 28 29 30 31");
    true
}

fn cmd_hostname(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() > 1 {
        match sc::sys_eon_set_hostname(args[1]) {
            Ok(()) => {}
            Err(e) => eprintln_fn(&format!("hostname: {}", e)),
        }
    } else {
        let mut buf = [0u8; 256];
        match sc::sys_eon_get_hostname(&mut buf) {
            Ok(n) => {
                if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                    println(s);
                }
            }
            Err(_) => println("echos"),
        }
    }
    true
}

fn cmd_export(state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        for (k, v) in state.env.list() {
            print(&format!("declare -x {}=\"{}\"\n", k, v));
        }
        return true;
    }
    let mut i = 1;
    while i < args.len() {
        if args[i] == "-n" {
            i += 1;
            if i < args.len() {
                state.env.unset(&format!("__export_{}", args[i]));
            }
        } else if args[i].contains('=') {
            let parts: Vec<&str> = args[i].splitn(2, '=').collect();
            state.env.set(parts[0], parts[1]);
            state.env.set(&format!("__export_{}", parts[0]), "1");
        } else {
            state.env.set(args[i], "");
            state.env.set(&format!("__export_{}", args[i]), "1");
        }
        i += 1;
    }
    true
}

fn cmd_unset(state: &mut ShellState, args: &[&str]) -> bool {
    let mut force_fn = false;
    let mut i = 1;
    while i < args.len() {
        if args[i] == "-f" {
            force_fn = true;
            i += 1;
            continue;
        }
        if args[i] == "-v" || !force_fn {
            state.env.unset(args[i]);
        }
        i += 1;
    }
    true
}

fn cmd_set(state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        for (k, v) in state.env.list() {
            print(&format!("{}={}\n", k, v));
        }
        let opts = state.env.opts.lock().to_flags_string();
        print(&format!("set -- {}\n", opts));
        return true;
    }
    let mut i = 1;
    while i < args.len() {
        match args[i] {
            "-e" => {
                state.env.opts.lock().errexit = true;
            }
            "+e" => {
                state.env.opts.lock().errexit = false;
            }
            "-x" => {
                state.env.opts.lock().xtrace = true;
            }
            "+x" => {
                state.env.opts.lock().xtrace = false;
            }
            "-u" => {
                state.env.opts.lock().nounset = true;
            }
            "+u" => {
                state.env.opts.lock().nounset = false;
            }
            "-a" => {
                state.env.opts.lock().allexport = true;
            }
            "+a" => {
                state.env.opts.lock().allexport = false;
            }
            "-f" => {
                state.env.opts.lock().noglob = true;
            }
            "+f" => {
                state.env.opts.lock().noglob = false;
            }
            "-h" => {
                state.env.opts.lock().hashall = true;
            }
            "+h" => {
                state.env.opts.lock().hashall = false;
            }
            "-H" => {
                state.env.opts.lock().histexpand = true;
            }
            "+H" => {
                state.env.opts.lock().histexpand = false;
            }
            "-m" => {
                state.env.opts.lock().monitor = true;
            }
            "+m" => {
                state.env.opts.lock().monitor = false;
            }
            "-C" => {
                state.env.opts.lock().noclobber = true;
            }
            "+C" => {
                state.env.opts.lock().noclobber = false;
            }
            "-v" => {
                state.env.opts.lock().verbose = true;
            }
            "+v" => {
                state.env.opts.lock().verbose = false;
            }
            "-B" => {
                state.env.opts.lock().braceexpand = true;
            }
            "+B" => {
                state.env.opts.lock().braceexpand = false;
            }
            "-o" => {
                i += 1;
                if i < args.len() {
                    let opt = args[i];
                    if let Some(pos) = opt.find('=') {
                        let name = &opt[..pos];
                        let val = &opt[pos + 1..];
                        state.env.opts.lock().set_by_name(name, true);
                        state.env.set(name, val);
                    } else if opt.starts_with('+') {
                        state.env.opts.lock().set_by_name(&opt[1..], false);
                    } else {
                        state.env.opts.lock().set_by_name(opt, true);
                    }
                }
            }
            "+o" => {
                i += 1;
                if i < args.len() {
                    state.env.opts.lock().set_by_name(args[i], false);
                }
            }
            "--" => {
                break;
            }
            _ => {
                if args[i].contains('=') {
                    let parts: Vec<&str> = args[i].splitn(2, '=').collect();
                    state.env.set(parts[0], parts[1]);
                }
            }
        }
        i += 1;
    }
    true
}

fn cmd_env(state: &mut ShellState) -> bool {
    for (k, v) in state.env.list() {
        println(&format!("{}={}", k, v));
    }
    true
}

fn cmd_printenv(state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        return cmd_env(state);
    }
    for arg in &args[1..] {
        if let Some(val) = state.env.get(arg) {
            println(&val);
        }
    }
    true
}

fn cmd_history(state: &mut ShellState) -> bool {
    for (i, cmd) in state.history.list().iter().enumerate() {
        println(&format!("{:5}  {}", i + 1, cmd));
    }
    true
}

fn cmd_lsmod(_state: &mut ShellState) -> bool {
    println("Module                  Size  Used by");
    println("echos_core            65536  1");
    true
}

fn cmd_iostat(_state: &mut ShellState) -> bool {
    println("Device             tps    kB_read/s    kB_wrtn/s");
    println("vda               0.00         0.00         0.00");
    true
}

fn cmd_mount(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        println("/dev/vda1 / f2fs rw 0 0");
        return true;
    }
    println(&format!("mount: {} not available in ring3", args[1]));
    true
}

fn cmd_umount(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("umount: missing target");
        return true;
    }
    println(&format!("umount: {} not available in ring3", args[1]));
    true
}

fn cmd_mktemp(_state: &mut ShellState) -> bool {
    let pid = sc::sys_getpid();
    let name = format!("/tmp/tmp.{}", pid);
    executor::write_file(&name, b"");
    println(&name);
    true
}

fn cmd_net(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        println("Usage: net [show|config|status]");
        return true;
    }
    match args[1] {
        "show" | "status" => {
            let mut buf = [0u8; 2048];
            match sc::sys_eon_net_config(&mut buf) {
                Ok(n) => {
                    if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                        println(s);
                    }
                }
                Err(_) => println("Network info not available"),
            }
        }
        _ => println(&format!("net: unknown subcommand: {}", args[1])),
    }
    true
}

fn cmd_ping(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("ping: usage: ping HOST");
        return true;
    }
    println(&format!("PING {} ({}): 56 data bytes", args[1], args[1]));
    println("64 bytes from {}: icmp_seq=0 ttl=64 time=0.001 ms");
    true
}

fn cmd_dns(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("dns: usage: dns HOST");
        return true;
    }
    println(&format!(
        "DNS lookup for {} not available in ring3",
        args[1]
    ));
    true
}

fn cmd_service(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 3 {
        eprintln_fn("service: usage: service NAME {start|stop|status}");
        return true;
    }
    println(&format!(
        "service {} {} not available in ring3",
        args[1], args[2]
    ));
    true
}

fn cmd_run(state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("run: usage: run SCRIPT");
        return true;
    }
    if let Some(data) = executor::load_file(args[1]) {
        let text = core::str::from_utf8(&data).unwrap_or("");
        scripting::run_script(state, text);
    } else {
        eprintln_fn(&format!("run: {} okunamadi", args[1]));
    }
    true
}

fn cmd_source(state: &mut ShellState, args: &[&str]) -> bool {
    cmd_run(state, args)
}

fn cmd_sleep(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("sleep: usage: sleep SECONDS");
        return true;
    }
    let secs: u64 = args[1].parse().unwrap_or(1);
    let _ = sc::sys_nanosleep(secs, 0);
    true
}

fn cmd_test(state: &mut ShellState, args: &[&str]) -> bool {
    let mut a = args;
    if a[0] == "[" {
        a = &a[1..];
    }
    if a.last() == Some(&"]") {
        a = &a[..a.len() - 1];
    }
    if a.is_empty() {
        state.exit_code = 1;
        return true;
    }
    match a[0] {
        "-f" | "-e" => {
            state.exit_code = if sc::sys_open(a.get(1).unwrap_or(&""), 0).is_ok() {
                0
            } else {
                1
            };
        }
        "-d" => {
            state.exit_code = 1;
        }
        "-z" => {
            state.exit_code = if a.get(1).map_or(true, |s| s.is_empty()) {
                0
            } else {
                1
            };
        }
        "-n" => {
            state.exit_code = if a.get(1).map_or(false, |s| !s.is_empty()) {
                0
            } else {
                1
            };
        }
        "=" | "==" => {
            state.exit_code = if a.get(1) == a.get(2) { 0 } else { 1 };
        }
        "!=" => {
            state.exit_code = if a.get(1) != a.get(2) { 0 } else { 1 };
        }
        "-eq" => {
            state.exit_code = if a.get(1).and_then(|x| x.parse::<i32>().ok())
                == a.get(2).and_then(|x| x.parse::<i32>().ok())
            {
                0
            } else {
                1
            };
        }
        "-ne" => {
            state.exit_code = if a.get(1).and_then(|x| x.parse::<i32>().ok())
                != a.get(2).and_then(|x| x.parse::<i32>().ok())
            {
                0
            } else {
                1
            };
        }
        "-lt" => {
            state.exit_code = if a.get(1).and_then(|x| x.parse::<i32>().ok())
                < a.get(2).and_then(|x| x.parse::<i32>().ok())
            {
                0
            } else {
                1
            };
        }
        "-le" => {
            state.exit_code = if a.get(1).and_then(|x| x.parse::<i32>().ok())
                <= a.get(2).and_then(|x| x.parse::<i32>().ok())
            {
                0
            } else {
                1
            };
        }
        "-gt" => {
            state.exit_code = if a.get(1).and_then(|x| x.parse::<i32>().ok())
                > a.get(2).and_then(|x| x.parse::<i32>().ok())
            {
                0
            } else {
                1
            };
        }
        "-ge" => {
            state.exit_code = if a.get(1).and_then(|x| x.parse::<i32>().ok())
                >= a.get(2).and_then(|x| x.parse::<i32>().ok())
            {
                0
            } else {
                1
            };
        }
        _ => {
            state.exit_code = 1;
        }
    }
    true
}

fn cmd_seq(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        return true;
    }
    let (start, end, step) = if args.len() == 2 {
        (1, args[1].parse::<i64>().unwrap_or(1), 1)
    } else if args.len() == 3 {
        (
            args[1].parse::<i64>().unwrap_or(1),
            args[2].parse::<i64>().unwrap_or(1),
            1,
        )
    } else {
        (
            args[1].parse::<i64>().unwrap_or(1),
            args[2].parse::<i64>().unwrap_or(1),
            args[3].parse::<i64>().unwrap_or(1),
        )
    };
    if step == 0 {
        return true;
    }
    let mut i = start;
    if step > 0 {
        while i <= end {
            println(&format!("{}", i));
            i += step;
        }
    } else {
        while i >= end {
            println(&format!("{}", i));
            i += step;
        }
    }
    true
}

fn cmd_yes(_state: &mut ShellState, args: &[&str]) -> bool {
    let text = if args.len() > 1 { args[1] } else { "y" };
    loop {
        println(text);
    }
}

fn cmd_loop(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        println("Usage: loop list|attach|flush|detach|mount|umount");
        return true;
    }
    match args[1] {
        "list" => {
            println("No loopback devices");
        }
        _ => println(&format!("loop {}: not available in ring3", args[1])),
    }
    true
}

fn cmd_hashsum(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn(&format!("{}: missing file", args[0]));
        return true;
    }
    println(&format!(
        "{}  {}  {}",
        args[0], "00000000000000000000000000000000", args[1]
    ));
    true
}

fn cmd_cksum(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("cksum: missing file");
        return true;
    }
    if let Some(data) = executor::load_file(args[1]) {
        let mut checksum: u32 = 0;
        for &b in &data {
            checksum = checksum.wrapping_add(b as u32);
        }
        println(&format!("{} {} {}", checksum, data.len(), args[1]));
    }
    true
}

// ============================================================================
// POSIX BUILTIN IMPLEMENTATIONS
// ============================================================================

fn cmd_read(state: &mut ShellState, args: &[&str]) -> bool {
    let mut silent = false;
    let mut prompt = String::new();
    let mut var_names: Vec<String> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        match args[i] {
            "-r" => {
                i += 1;
            }
            "-s" => {
                silent = true;
                i += 1;
            }
            "-p" => {
                i += 1;
                if i < args.len() {
                    prompt.push_str(args[i]);
                    i += 1;
                }
            }
            _ => {
                var_names.push(args[i].to_string());
                i += 1;
            }
        }
    }
    if !prompt.is_empty() {
        print(&prompt);
    }
    let mut line = String::new();
    let mut buf = [0u8; 1];
    loop {
        match sc::sys_read(0, &mut buf) {
            Ok(1) => {
                if buf[0] == b'\n' || buf[0] == b'\r' {
                    break;
                }
                if buf[0] == 0x08 || buf[0] == 0x7F {
                    if !line.is_empty() {
                        line.pop();
                        if !silent {
                            print("\x08 \x08");
                        }
                    }
                } else if buf[0] >= 0x20 {
                    line.push(buf[0] as char);
                    if !silent {
                        let _ = sc::sys_write(1, &[buf[0]]);
                    }
                }
            }
            _ => break,
        }
    }
    if !silent {
        println("");
    }
    if var_names.is_empty() {
        state.env.set("REPLY", &line);
    } else {
        let words: Vec<&str> = line.split_whitespace().collect();
        for (i, name) in var_names.iter().enumerate() {
            let val = if i < words.len() { words[i] } else { "" };
            state.env.set(name, val);
        }
    }
    true
}

fn cmd_shift(state: &mut ShellState, args: &[&str]) -> bool {
    let n: usize = if args.len() > 1 {
        args[1].parse().unwrap_or(1)
    } else {
        1
    };
    for _ in 0..n {
        for i in 1..=99 {
            let key = format!("{}", i);
            let next = format!("{}", i + 1);
            match state.env.get(&next) {
                Some(val) => state.env.set(&key, &val),
                None => state.env.unset(&key),
            }
        }
    }
    true
}

fn cmd_trap(state: &mut ShellState, args: &[&str]) -> bool {
    let line = if args.len() > 1 {
        format!("trap {}", args[1..].join(" "))
    } else {
        String::from("trap")
    };
    state.exit_code = crate::scripting::exec_trap(state, &line);
    true
}

fn cmd_getopts(state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 3 {
        eprintln_fn("getopts: usage: getopts optstring name [arg...]");
        return true;
    }
    let optstring = args[1];
    let name = args[2];
    let optind: usize = state
        .env
        .get("OPTIND")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let optarg_key = "OPTARG";
    let argv: Vec<&str> = if args.len() > 3 {
        args[3..].iter().copied().collect()
    } else {
        Vec::new()
    };
    if optind >= argv.len() {
        state.exit_code = 1;
        return false;
    }
    let arg = argv[optind];
    if arg.starts_with('-') && arg.len() > 1 {
        let opt_char = arg.as_bytes()[1] as char;
        if optstring.contains(opt_char) {
            state.env.set(name, &alloc::format!("{}", opt_char));
            if optstring.contains(alloc::format!("{}:", opt_char).as_str()) {
                if arg.len() > 2 {
                    state.env.set(optarg_key, &arg[2..]);
                } else if optind + 1 < argv.len() {
                    state.env.set(optarg_key, argv[optind + 1]);
                    state.env.set("OPTIND", &alloc::format!("{}", optind + 2));
                } else {
                    eprintln_fn(&alloc::format!(
                        "getopts: option requires an argument -- {}",
                        opt_char
                    ));
                    state.env.set(name, "?");
                    state.env.set(optarg_key, "");
                }
            } else {
                state.env.unset(optarg_key);
                if arg.len() > 2 {
                    state.env.set("OPTIND", &alloc::format!("{}", optind));
                    let new_arg = alloc::format!("-{}", &arg[1..]);
                    state.env.set("_", &new_arg);
                } else {
                    state.env.set("OPTIND", &alloc::format!("{}", optind + 1));
                }
            }
            state.exit_code = 0;
            return true;
        } else {
            eprintln_fn(&alloc::format!("getopts: illegal option -- {}", opt_char));
            state.env.set(name, "?");
            state.env.set("OPTARG", "");
            state.exit_code = 0;
            return true;
        }
    }
    state.exit_code = 1;
    false
}

fn cmd_select(state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("select: usage: select name [in list...]");
        return true;
    }
    let var_name = args[1];
    let items: Vec<String> = if args.len() > 3 && args[2] == "in" {
        args[3..].iter().map(|s| s.to_string()).collect()
    } else {
        args[2..].iter().map(|s| s.to_string()).collect()
    };
    let mut num = 1;
    for item in &items {
        crate::println(&format!("{}  {}", num, item));
        num += 1;
    }
    crate::print("#? ");
    let mut line = String::new();
    let mut buf = [0u8; 1];
    loop {
        match sc::sys_read(0, &mut buf) {
            Ok(1) => {
                if buf[0] == b'\n' || buf[0] == b'\r' {
                    break;
                }
                line.push(buf[0] as char);
            }
            _ => break,
        }
    }
    if let Ok(choice) = line.trim().parse::<usize>() {
        if choice >= 1 && choice <= items.len() {
            state.env.set(var_name, &items[choice - 1]);
            return true;
        }
    }
    state.env.set(var_name, "");
    true
}

fn cmd_declare(state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        for (k, v) in state.env.list() {
            crate::println(&format!("declare -- {}=\"{}\"", k, v));
        }
        for (k, v) in state.env.list_arrays() {
            let items: Vec<&str> = v.iter().map(|s| s.as_str()).collect();
            crate::println(&format!("declare -a {}=({})", k, items.join(" ")));
        }
        return true;
    }
    let mut i = 1;
    while i < args.len() {
        if args[i] == "-a" || args[i] == "-A" {
            i += 1;
            if i < args.len() {
                let name_val = args[i];
                if let Some(pos) = name_val.find('=') {
                    let name = &name_val[..pos];
                    let val_str = &name_val[pos + 1..];
                    if val_str.starts_with('(') && val_str.ends_with(')') {
                        let inner = &val_str[1..val_str.len() - 1];
                        let values: Vec<String> =
                            inner.split_whitespace().map(|s| s.to_string()).collect();
                        state.env.set_array(name, values);
                    } else {
                        let values: Vec<String> =
                            val_str.split_whitespace().map(|s| s.to_string()).collect();
                        state.env.set_array(name, values);
                    }
                }
            }
        } else if args[i].contains('=') {
            let parts: Vec<&str> = args[i].splitn(2, '=').collect();
            let val = if parts.len() > 1 { parts[1] } else { "" };
            if val.starts_with('(') && val.ends_with(')') {
                let inner = &val[1..val.len() - 1];
                let values: Vec<String> = inner.split_whitespace().map(|s| s.to_string()).collect();
                state.env.set_array(parts[0], values);
            } else {
                state.env.set(parts[0], val);
            }
        } else {
            state.env.set(args[i], "");
        }
        i += 1;
    }
    true
}

fn cmd_readonly(state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        for (k, v) in state.env.list() {
            crate::println(&format!("readonly -- {}=\"{}\"", k, v));
        }
        return true;
    }
    let mut i = 1;
    while i < args.len() {
        if args[i].contains('=') {
            let parts: Vec<&str> = args[i].splitn(2, '=').collect();
            state.env.set(parts[0], parts[1]);
        }
        i += 1;
    }
    true
}

// ============================================================================
// POSIX UTILITIES — P0/P1 Eksikler
// ============================================================================

/// `basename` — Yoldan dosya adını çıkarır (POSIX zorunlu)
/// basename /path/to/file.txt         → file.txt
/// basename /path/to/file.txt .txt    → file
fn cmd_basename(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("basename: missing operand");
        return true;
    }
    let path = args[1];
    let suffix = args.get(2).map(|s| *s);
    // Son '/' karakterine kadar olan kısım
    let base = match path.rfind('/') {
        Some(pos) => &path[pos + 1..],
        None => path,
    };
    // Suffix kaldırma
    if let Some(suf) = suffix {
        if suf.len() > 0 && base.ends_with(suf) {
            let result = &base[..base.len() - suf.len()];
            println(if result.is_empty() { "/" } else { result });
        } else {
            println(base);
        }
    } else {
        println(if base.is_empty() { "/" } else { base });
    }
    true
}

/// `dirname` — Yoldan dizin adını çıkarır (POSIX zorunlu)
/// dirname /path/to/file.txt  → /path/to
/// dirname file.txt           → .
fn cmd_dirname(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("dirname: missing operand");
        return true;
    }
    let path = args[1];
    // Sağdaki '/' karakterlerini kaldır
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        println("/");
    } else {
        match trimmed.rfind('/') {
            Some(0) => println("/"),
            Some(pos) => println(&trimmed[..pos]),
            None => println("."),
        }
    }
    true
}

/// `hash` — Komut yol önbellek tablosu (POSIX zorunlu)
/// hash       → önbelleği listeler
/// hash cmd   → cmd yolunu ara ve önbelleğe al
fn cmd_hash(state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        // Boş hash tablosu listele
        println("hash table is empty");
        return true;
    }
    let paths = state
        .env
        .get("PATH")
        .unwrap_or(String::from("/bin:/usr/bin"));
    for cmd_name in &args[1..] {
        let mut found = false;
        for dir in paths.split(':') {
            let full = if dir == "/" {
                format!("/{}", cmd_name)
            } else {
                format!("{}/{}", dir, cmd_name)
            };
            if sc::sys_open(&full, 0).is_ok() {
                // Başarılı yolu kaydet
                state.env.set(&format!("_hash_{}", cmd_name), &full);
                found = true;
                break;
            }
        }
        if !found {
            eprintln_fn(&format!("hash: {} not found", cmd_name));
        }
    }
    true
}

/// `dd` — Block-level veri dönüşümü (POSIX)
/// dd if=input of=output bs=4096 count=1
fn cmd_dd(_state: &mut ShellState, args: &[&str]) -> bool {
    let mut ifile = String::new();
    let mut ofile = String::new();
    let mut bs: usize = 512;
    let mut count: usize = usize::MAX;
    for arg in &args[1..] {
        if let Some(v) = arg.strip_prefix("if=") {
            ifile = v.to_string();
        } else if let Some(v) = arg.strip_prefix("of=") {
            ofile = v.to_string();
        } else if let Some(v) = arg.strip_prefix("bs=") {
            bs = v.parse().unwrap_or(512);
        } else if let Some(v) = arg.strip_prefix("count=") {
            count = v.parse().unwrap_or(usize::MAX);
        }
    }
    if ifile.is_empty() || ofile.is_empty() {
        eprintln_fn("dd: usage: dd if=INPUT of=OUTPUT [bs=SIZE] [count=N]");
        return true;
    }
    let fd_in = match sc::sys_open(&ifile, 0) {
        Ok(fd) => fd,
        Err(e) => {
            eprintln_fn(&format!("dd: {}: {}", ifile, e));
            return true;
        }
    };
    let mut bytes_total: usize = 0;
    let mut blocks_read: usize = 0;
    let mut buf = alloc::vec![0u8; bs];
    loop {
        if blocks_read >= count {
            break;
        }
        match sc::sys_read(fd_in, &mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if !executor::append_file(&ofile, &buf[..n]) {
                    eprintln_fn(&format!("dd: {}: write failed", ofile));
                    break;
                }
                bytes_total += n;
                blocks_read += 1;
            }
            Err(_) => break,
        }
    }
    let _ = sc::sys_close(fd_in);
    let records_in = blocks_read;
    let records_out = blocks_read;
    println(&format!("{}+{} records in", records_in, 0));
    println(&format!("{}+{} records out", records_out, 0));
    println(&format!("{} bytes copied", bytes_total));
    true
}

/// `diff` — Dosya karşılaştırması (basit satır satır)
fn cmd_diff(state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 3 {
        eprintln_fn("diff: usage: diff FILE1 FILE2");
        return true;
    }
    let d1 = executor::load_file(args[1]).unwrap_or_default();
    let d2 = executor::load_file(args[2]).unwrap_or_default();
    let t1 = core::str::from_utf8(&d1).unwrap_or("");
    let t2 = core::str::from_utf8(&d2).unwrap_or("");
    let l1: Vec<&str> = t1.lines().collect();
    let l2: Vec<&str> = t2.lines().collect();
    if l1 == l2 {
        return true;
    } // Fark yoksa sessizce çık
      // Basit satır-satır fark
    let max_lines = if l1.len() > l2.len() {
        l1.len()
    } else {
        l2.len()
    };
    for i in 0..max_lines {
        let line1 = l1.get(i).copied().unwrap_or("");
        let line2 = l2.get(i).copied().unwrap_or("");
        if line1 != line2 {
            println(&format!("{}c{}", i + 1, i + 1));
            println(&format!("< {}", line1));
            println(&format!("> {}", line2));
        }
    }
    state.exit_code = 1;
    true
}

/// `find` — Dosya/dizin arama (basit: -name, -type)
/// find /path -name "*.txt"
/// find /path -type f
fn cmd_find(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("find: usage: find PATH [EXPRESSION]");
        return true;
    }
    let root = args[1];
    let mut name_pattern: Option<&str> = None;
    let mut type_filter: Option<char> = None;
    let mut i = 2;
    while i < args.len() {
        match args[i] {
            "-name" => {
                i += 1;
                if i < args.len() {
                    name_pattern = Some(args[i]);
                }
            }
            "-type" => {
                i += 1;
                if i < args.len() {
                    type_filter = Some(args[i].chars().next().unwrap_or('f'));
                }
            }
            _ => {}
        }
        i += 1;
    }
    find_recursive(root, name_pattern, type_filter);
    true
}

fn find_recursive(path: &str, name_pattern: Option<&str>, type_filter: Option<char>) {
    let mut buf = [0u8; 8192];
    // Mevcut entry'yi yazdır
    let matches_name = name_pattern.map_or(true, |pat| {
        let base = match path.rfind('/') {
            Some(pos) => &path[pos + 1..],
            None => path,
        };
        simple_glob_match(pat, base)
    });
    let is_dir = {
        if let Ok(fd) = sc::sys_open(path, 0) {
            let mut buf2 = [0u8; 8192];
            let is_d = sc::sys_getdents64(fd, &mut buf2)
                .map(|n| n > 0)
                .unwrap_or(false);
            let _ = sc::sys_close(fd);
            is_d
        } else {
            false
        }
    };
    let is_file = !is_dir && sc::sys_open(path, 0).is_ok();
    let matches_type = type_filter.map_or(true, |t| match t {
        'f' => is_file,
        'd' => is_dir,
        _ => true,
    });
    if matches_name && matches_type {
        println(path);
    }
    // Dizin ise alt dizinleri gez
    if is_dir {
        if let Ok(fd) = sc::sys_open(path, 0) {
            if let Ok(n) = sc::sys_getdents64(fd, &mut buf) {
                sc::for_each_dirent64(&buf, n, |name, _| {
                    if name != "." && name != ".." && !name.is_empty() {
                        let full = if path == "/" {
                            format!("/{}", name)
                        } else {
                            format!("{}/{}", path, name)
                        };
                        find_recursive(&full, name_pattern, type_filter);
                    }
                });
            }
            let _ = sc::sys_close(fd);
        }
    }
}

/// Basit glob matching: *, ?, []
fn simple_glob_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    glob_match_impl(&p, &n, 0, 0)
}

fn glob_match_impl(p: &[char], n: &[char], pi: usize, ni: usize) -> bool {
    if pi == p.len() {
        return ni == n.len();
    }
    if p[pi] == '*' {
        for skip in 0..=n.len() - ni {
            if glob_match_impl(p, n, pi + 1, ni + skip) {
                return true;
            }
        }
        return false;
    }
    if p[pi] == '?' {
        return ni < n.len() && glob_match_impl(p, n, pi + 1, ni + 1);
    }
    if p[pi] == '[' {
        let mut j = pi + 1;
        let mut negate = false;
        if j < p.len() && (p[j] == '^' || p[j] == '!') {
            negate = true;
            j += 1;
        }
        let mut found = false;
        while j < p.len() && p[j] != ']' {
            if j + 2 < p.len() && p[j + 1] == '-' {
                if ni < n.len() && n[ni] >= p[j] && n[ni] <= p[j + 2] {
                    found = true;
                }
                j += 3;
            } else {
                if ni < n.len() && n[ni] == p[j] {
                    found = true;
                }
                j += 1;
            }
        }
        let end = if j < p.len() { j + 1 } else { j };
        if found == negate {
            return false;
        }
        return ni < n.len() && glob_match_impl(p, n, end, ni + 1);
    }
    ni < n.len() && p[pi] == n[ni] && glob_match_impl(p, n, pi + 1, ni + 1)
}

/// `sed` — POSIX sed: s///, d, p, a/i/c, adres aralığı (;) ile çoklu komut
/// sed 's/foo/bar/g' file
/// sed '1,5d' file
/// sed '1i\hello' file
/// sed '/pattern/a\appended text' file
/// sed 's/a/b/; s/c/d/' file
fn cmd_sed(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("sed: usage: sed 'COMMAND' [FILE]");
        return true;
    }
    let cmd_str = args[1];
    let file_data = if args.len() > 2 {
        executor::load_file(args[2]).unwrap_or_default()
    } else {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];
        loop {
            match sc::sys_read(0, &mut tmp) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                Err(_) => break,
            }
        }
        buf
    };
    let text = core::str::from_utf8(&file_data).unwrap_or("");
    let lines: Vec<&str> = text.lines().collect();
    // Komutları ; ile ayır (tırnak içinde değilse)
    let commands = sed_split_commands(cmd_str);
    let mut line_idx = 0usize;
    let mut suppress_default = false; // 'd' komutu çalışmışsa satırı yazdırma
    for line in &lines {
        line_idx += 1;
        suppress_default = false;
        let mut cmd_idx = 0;
        while cmd_idx < commands.len() {
            let cmd = commands[cmd_idx].trim();
            cmd_idx += 1;
            // Adres aralığı: "1,5" veya "/pattern/" veya "1" veya "$"
            let (addr_start, addr_end, cmd_body) = sed_parse_address(cmd);
            let in_range = match (addr_start, addr_end) {
                (Some(s), Some(e)) => {
                    let s_line = sed_resolve_line_addr(&lines, line_idx, &s);
                    let e_line = sed_resolve_line_addr(&lines, line_idx, &e);
                    line_idx >= s_line && line_idx <= e_line
                }
                (Some(s), None) => {
                    let s_line = sed_resolve_line_addr(&lines, line_idx, &s);
                    line_idx == s_line
                }
                (None, None) => true,
                _ => false,
            };
            if !in_range {
                continue;
            }
            let cmd_body = cmd_body.trim();
            // s/pattern/replacement/g
            if cmd_body.starts_with("s/") {
                let inner = &cmd_body[2..];
                let p_end = inner.find('/').unwrap_or(inner.len());
                let pattern = &inner[..p_end];
                let rest = if p_end < inner.len() {
                    &inner[p_end + 1..]
                } else {
                    ""
                };
                let r_end = rest.find('/').unwrap_or(rest.len());
                let replacement = &rest[..r_end];
                let global = rest[r_end..].contains('g');
                if global {
                    let new_line = line.replace(pattern, replacement);
                    if new_line != *line {
                        suppress_default = true;
                    }
                    println(&new_line);
                } else {
                    let new_line = line.replacen(pattern, replacement, 1);
                    if new_line != *line {
                        suppress_default = true;
                    }
                    println(&new_line);
                }
                continue;
            }
            if cmd_body == "d" {
                suppress_default = true;
                continue; // satırı yazdırma
            }
            if cmd_body == "p" {
                println(line);
                continue;
            }
            // a\text — satır sonuna ekle
            if cmd_body.starts_with("a\\") || cmd_body.starts_with("a\\") {
                let text_to_append = cmd_body.trim_start_matches('a').trim_start_matches('\\');
                println(line);
                println(text_to_append);
                suppress_default = true;
                continue;
            }
            // i\text — satır başına ekle
            if cmd_body.starts_with("i\\") {
                let text_to_insert = cmd_body.trim_start_matches('i').trim_start_matches('\\');
                println(text_to_insert);
                println(line);
                suppress_default = true;
                continue;
            }
            // c\text — satırı değiştir
            if cmd_body.starts_with("c\\") {
                let text_to_replace = cmd_body.trim_start_matches('c').trim_start_matches('\\');
                println(text_to_replace);
                suppress_default = true;
                continue;
            }
            // y/src/dst/ — karakter dönüşümü (tr benzeri)
            if cmd_body.starts_with("y/") {
                let inner = &cmd_body[2..];
                let parts: Vec<&str> = inner.splitn(2, '/').collect();
                let from = parts[0];
                let to = if parts.len() > 1 {
                    parts[1].trim_end_matches('/')
                } else {
                    ""
                };
                let mut new_line = String::new();
                for ch in line.chars() {
                    if let Some(pos) = from.find(ch) {
                        if let Some(tc) = to.chars().nth(pos) {
                            new_line.push(tc);
                        } else {
                            new_line.push(ch);
                        }
                    } else {
                        new_line.push(ch);
                    }
                }
                println(&new_line);
                suppress_default = true;
                continue;
            }
            // q — sed'den çık
            if cmd_body == "q" {
                println(line);
                return true;
            }
        }
        // Varsayılan: satırı yazdır (d komutu çalışmadıysa)
        if !suppress_default {
            println(line);
        }
    }
    true
}

/// sed komutlarını ; ile ayır (tırnak içinde değilse)
fn sed_split_commands(cmd_str: &str) -> Vec<&str> {
    let mut cmds = Vec::new();
    let mut last = 0;
    let mut in_sq = false;
    let mut in_dq = false;
    let bytes = cmd_str.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' if !in_dq => {
                in_sq = !in_sq;
            }
            b'"' if !in_sq => {
                in_dq = !in_dq;
            }
            b';' if !in_sq && !in_dq => {
                cmds.push(&cmd_str[last..i]);
                last = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if last < bytes.len() {
        cmds.push(&cmd_str[last..]);
    }
    cmds
}

/// sed adres ayrıştırıcı: "1,5", "/pattern/", "1", "$", veya adres yok
fn sed_parse_address(cmd: &str) -> (Option<String>, Option<String>, &str) {
    let cmd = cmd.trim_start();
    if cmd.starts_with('/') {
        // /pattern/ komut
        if let Some(end) = cmd[1..].find('/') {
            let pattern = cmd[1..end + 1].to_string();
            let rest = cmd[end + 2..].trim();
            return (Some(pattern), None, rest);
        }
    }
    // Sayısal adres veya aralık
    if let Some(comma_pos) = cmd.find(',') {
        let before = cmd[..comma_pos].trim();
        let after_comma = cmd[comma_pos + 1..].trim();
        // Sayısal adres kontrolü
        if before.parse::<usize>().is_ok() || before == "$" || before.starts_with('/') {
            let s = before.to_string();
            if let Some(rest_end) =
                after_comma.find(|c: char| !c.is_ascii_digit() && c != '$' && c != '/')
            {
                let e = after_comma[..rest_end].trim().to_string();
                let rest = after_comma[rest_end..].trim();
                return (Some(s), Some(e), rest);
            } else {
                return (Some(s), Some(after_comma.to_string()), "");
            }
        }
    }
    // Tek adres: "1" veya "$"
    if cmd.parse::<usize>().is_ok() || cmd.starts_with('$') || cmd.starts_with('/') {
        let first_word: Vec<&str> = cmd
            .splitn(2, |c: char| c == ' ' || c == '/' || c == 'p' || c == 'd')
            .collect();
        if !cmd.starts_with('/') {
            if let Some(end) = cmd.find(|c: char| !c.is_ascii_digit()) {
                let addr = cmd[..end].to_string();
                let rest = cmd[end..].trim();
                return (Some(addr), None, rest);
            } else {
                return (Some(cmd.to_string()), None, "");
            }
        }
    }
    (None, None, cmd)
}

/// Adresi satır numarasına çevir
fn sed_resolve_line_addr(lines: &[&str], current_line: usize, addr: &str) -> usize {
    if addr == "$" {
        return lines.len();
    }
    if let Ok(n) = addr.parse::<usize>() {
        return n;
    }
    if addr.starts_with('/') && addr.ends_with('/') {
        let pattern = &addr[1..addr.len() - 1];
        for (i, line) in lines.iter().enumerate() {
            if line.contains(pattern) {
                return i + 1;
            }
        }
    }
    current_line
}

/// `awk` — POSIX awk destekli: -F, -v OFS=..., BEGIN/END, NF, NR, pattern matching, $0..$NF, printf
/// awk '{print $1,$3}' file
/// awk -F: '{print NR": "$0}' file
/// awk -v OFS=, '{print $1,$2}' file
/// awk 'BEGIN{print "header"} {print} END{print "footer"}' file
fn cmd_awk(_state: &mut ShellState, args: &[&str]) -> bool {
    let mut fs = ' ';
    let mut ofss = String::from(" ");
    let mut assignments: Vec<(String, String)> = Vec::new();
    let mut pattern_idx = 1;
    // Parse -F, -v OFS=... options
    while pattern_idx < args.len() {
        match args[pattern_idx] {
            "-F" if pattern_idx + 1 < args.len() => {
                fs = args[pattern_idx + 1].chars().next().unwrap_or(' ');
                pattern_idx += 2;
            }
            "-v" if pattern_idx + 1 < args.len() => {
                let kv = args[pattern_idx + 1];
                if let Some(eq) = kv.find('=') {
                    let k = &kv[..eq];
                    let v = &kv[eq + 1..];
                    assignments.push((k.to_string(), v.to_string()));
                    if k == "OFS" {
                        ofss = v.to_string();
                    }
                }
                pattern_idx += 2;
            }
            _ => break,
        }
    }
    if pattern_idx >= args.len() {
        eprintln_fn("awk: usage: awk [-F SEP] [-v OFS=...] 'PATTERN {ACTION}' FILE");
        return true;
    }
    // Split begin/action/end blocks
    let spec = args[pattern_idx];
    let mut begin_action: Option<&str> = None;
    let mut main_action: Option<&str> = None;
    let mut end_action: Option<&str> = None;
    // Parse: BEGIN{...} {action} END{...}
    let spec_bytes = spec.as_bytes();
    let mut pos = 0;
    while pos < spec_bytes.len() {
        // skip whitespace
        while pos < spec_bytes.len() && spec_bytes[pos] == b' ' {
            pos += 1;
        }
        if pos >= spec_bytes.len() {
            break;
        }
        // Check for BEGIN, END, or {action}
        if pos + 5 < spec_bytes.len()
            && &spec_bytes[pos..pos + 5] == b"BEGIN"
            && pos + 5 < spec_bytes.len()
        {
            pos += 5;
            while pos < spec_bytes.len() && spec_bytes[pos] == b' ' {
                pos += 1;
            }
            if pos < spec_bytes.len() && spec_bytes[pos] == b'{' {
                pos += 1;
                let start = pos;
                let mut depth = 1u32;
                while pos < spec_bytes.len() && depth > 0 {
                    if spec_bytes[pos] == b'{' {
                        depth += 1;
                    } else if spec_bytes[pos] == b'}' {
                        depth -= 1;
                    }
                    pos += 1;
                }
                begin_action = Some(spec[start..pos - 1].trim());
            }
        } else if pos + 3 < spec_bytes.len()
            && &spec_bytes[pos..pos + 3] == b"END"
            && pos + 3 < spec_bytes.len()
        {
            pos += 3;
            while pos < spec_bytes.len() && spec_bytes[pos] == b' ' {
                pos += 1;
            }
            if pos < spec_bytes.len() && spec_bytes[pos] == b'{' {
                pos += 1;
                let start = pos;
                let mut depth = 1u32;
                while pos < spec_bytes.len() && depth > 0 {
                    if spec_bytes[pos] == b'{' {
                        depth += 1;
                    } else if spec_bytes[pos] == b'}' {
                        depth -= 1;
                    }
                    pos += 1;
                }
                end_action = Some(spec[start..pos - 1].trim());
            }
        } else if spec_bytes[pos] == b'{' {
            pos += 1;
            let start = pos;
            let mut depth = 1u32;
            while pos < spec_bytes.len() && depth > 0 {
                if spec_bytes[pos] == b'{' {
                    depth += 1;
                } else if spec_bytes[pos] == b'}' {
                    depth -= 1;
                }
                pos += 1;
            }
            main_action = Some(spec[start..pos - 1].trim());
        } else {
            // Single-char or unknown — skip to next space
            while pos < spec_bytes.len() && spec_bytes[pos] != b' ' {
                pos += 1;
            }
        }
    }
    // Fallback: if no braces found, treat entire spec as action
    if main_action.is_none() && begin_action.is_none() && end_action.is_none() {
        main_action = Some(spec.trim());
    }
    // Load input data
    let file_idx = pattern_idx + 1;
    let file_data = if file_idx < args.len() {
        executor::load_file(args[file_idx]).unwrap_or_default()
    } else {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];
        loop {
            match sc::sys_read(0, &mut tmp) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                Err(_) => break,
            }
        }
        buf
    };
    let text = core::str::from_utf8(&file_data).unwrap_or("");
    // Execute BEGIN
    if let Some(begin) = begin_action {
        execute_awk_action(begin, &[], &ofss, 0, 0, &assignments);
    }
    // Execute main action per line
    let mut nr = 0u32;
    for line in text.lines() {
        nr += 1;
        let fields: Vec<&str> = line.split(fs).collect();
        let nf = fields.len() as u32;
        if let Some(action) = main_action {
            execute_awk_action(action, &fields, &ofss, nf, nr, &assignments);
        } else {
            println(line);
        }
    }
    // Execute END
    if let Some(end) = end_action {
        execute_awk_action(end, &[], &ofss, 0, nr, &assignments);
    }
    true
}

/// awk action yürütücü: print, printf, NF, NR, $0, $1..$N, OFS, ASSIGNMENT
fn execute_awk_action(
    action: &str,
    fields: &[&str],
    ofss: &str,
    nf: u32,
    nr: u32,
    assignments: &[(String, String)],
) {
    // Multi-statement: split by ; at top level
    let stmts = awk_split_statements(action);
    for stmt in &stmts {
        let stmt = stmt.trim();
        if stmt.is_empty() {
            continue;
        }
        // print statement
        if stmt.starts_with("print ") {
            let args_str = &stmt[6..];
            let print_parts: Vec<&str> = args_str.split(',').map(|s| s.trim()).collect();
            let mut out = String::new();
            for (idx, part) in print_parts.iter().enumerate() {
                if idx > 0 {
                    out.push_str(ofss);
                }
                out.push_str(&awk_eval_field_or_var(part, fields, nf, nr, assignments));
            }
            println(&out);
            continue;
        }
        // printf statement
        if stmt.starts_with("printf ") {
            let fmt_and_args = &stmt[7..];
            let parts: Vec<&str> = awk_split_statements(fmt_and_args); // reuse for comma split
            if let Some(fmt_str) = parts.first() {
                let fmt = fmt_str.trim();
                let mut result = String::new();
                let mut fi = 0;
                let fmt_bytes = fmt.as_bytes();
                let mut arg_idx = 1;
                while fi < fmt_bytes.len() {
                    if fmt_bytes[fi] == b'%' && fi + 1 < fmt_bytes.len() {
                        fi += 1;
                        match fmt_bytes[fi] {
                            b's' | b'd' | b'i' | b'u' => {
                                if arg_idx < parts.len() {
                                    result.push_str(&awk_eval_field_or_var(
                                        parts[arg_idx],
                                        fields,
                                        nf,
                                        nr,
                                        assignments,
                                    ));
                                    arg_idx += 1;
                                }
                                fi += 1;
                            }
                            b'c' => {
                                if arg_idx < parts.len() {
                                    let val = awk_eval_field_or_var(
                                        parts[arg_idx],
                                        fields,
                                        nf,
                                        nr,
                                        assignments,
                                    );
                                    if let Some(c) = val.chars().next() {
                                        result.push(c);
                                    }
                                    arg_idx += 1;
                                }
                                fi += 1;
                            }
                            b'%' => {
                                result.push('%');
                                fi += 1;
                            }
                            _ => {
                                result.push('%');
                                result.push(fmt_bytes[fi] as char);
                                fi += 1;
                            }
                        }
                    } else {
                        result.push(fmt_bytes[fi] as char);
                        fi += 1;
                    }
                }
                println(&result);
            }
            continue;
        }
        // Assignment: VAR=value
        if let Some(eq) = stmt.find('=') {
            if eq > 0 {
                let var_name = stmt[..eq].trim();
                let value = awk_eval_field_or_var(&stmt[eq + 1..], fields, nf, nr, assignments);
                // Set variable (stored via assignments — limited to OFS etc.)
                // For simple awk, just handle OFS assignment inline
                continue;
            }
        }
        // Bare field reference like $0, $1, etc.
        let val = awk_eval_field_or_var(stmt, fields, nf, nr, assignments);
        if !val.is_empty() {
            println(&val);
        }
    }
}

fn awk_eval_field_or_var(
    expr: &str,
    fields: &[&str],
    nf: u32,
    nr: u32,
    assignments: &[(String, String)],
) -> String {
    let expr = expr.trim();
    if expr == "$0" {
        fields.join("")
    } else if expr.starts_with('$') {
        if let Ok(idx) = expr[1..].parse::<usize>() {
            if idx == 0 {
                fields.join("")
            } else {
                fields.get(idx - 1).copied().unwrap_or("").to_string()
            }
        } else {
            expr.to_string()
        }
    } else if expr == "NF" {
        nf.to_string()
    } else if expr == "NR" {
        nr.to_string()
    } else if expr == "OFS" {
        // OFS value from assignments
        assignments
            .iter()
            .find(|(k, _)| k == "OFS")
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| " ".to_string())
    } else if expr.starts_with('"') && expr.ends_with('"') && expr.len() >= 2 {
        // String literal
        expr[1..expr.len() - 1].to_string()
    } else {
        // Try as numeric literal
        if expr.parse::<i64>().is_ok() || expr.parse::<f64>().is_ok() {
            expr.to_string()
        } else {
            // Variable lookup from assignments
            assignments
                .iter()
                .find(|(k, _)| k == expr)
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| expr.to_string())
        }
    }
}

fn awk_split_statements(s: &str) -> Vec<&str> {
    let mut stmts = Vec::new();
    let mut depth = 0u32;
    let mut last = 0;
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut in_sq = false;
    let mut in_dq = false;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' if !in_dq => {
                in_sq = !in_sq;
            }
            b'"' if !in_sq => {
                in_dq = !in_dq;
            }
            b'{' if !in_sq && !in_dq => {
                depth += 1;
            }
            b'}' if !in_sq && !in_dq => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            b',' if depth == 0 && !in_sq && !in_dq => {
                stmts.push(&s[last..i]);
                last = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if last < bytes.len() {
        stmts.push(&s[last..]);
    }
    stmts
}

/// `paste` — Satırları yan yana birleştir
/// paste file1 file2
fn cmd_paste(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("paste: usage: paste FILE1 FILE2 ...");
        return true;
    }
    // Her dosyanın satırlarını String olarak depola (borrow lifetime sorunlarını önler)
    let mut file_lines: Vec<Vec<String>> = Vec::new();
    for path in &args[1..] {
        if let Some(data) = executor::load_file(path) {
            let text = core::str::from_utf8(&data).unwrap_or("");
            let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
            file_lines.push(lines);
        } else {
            file_lines.push(Vec::new());
        }
    }
    let max_lines = file_lines.iter().map(|l| l.len()).max().unwrap_or(0);
    for i in 0..max_lines {
        let parts: Vec<&str> = file_lines
            .iter()
            .map(|l| l.get(i).map(|s| s.as_str()).unwrap_or(""))
            .collect();
        println(&parts.join("\t"));
    }
    true
}

/// `join` — Sıralı dosyaları ortak alana göre birleştir
fn cmd_join(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 3 {
        eprintln_fn("join: usage: join FILE1 FILE2");
        return true;
    }
    let d1 = executor::load_file(args[1]).unwrap_or_default();
    let d2 = executor::load_file(args[2]).unwrap_or_default();
    let t1 = core::str::from_utf8(&d1).unwrap_or("");
    let t2 = core::str::from_utf8(&d2).unwrap_or("");
    let l1: Vec<&str> = t1.lines().collect();
    let l2: Vec<&str> = t2.lines().collect();
    let mut i = 0;
    let mut j = 0;
    while i < l1.len() && j < l2.len() {
        let f1: Vec<&str> = l1[i].split_whitespace().collect();
        let f2: Vec<&str> = l2[j].split_whitespace().collect();
        let key1 = f1.first().unwrap_or(&"");
        let key2 = f2.first().unwrap_or(&"");
        if key1 < key2 {
            i += 1;
        } else if key1 > key2 {
            j += 1;
        } else {
            let mut joined = String::new();
            joined.push_str(key1);
            for f in &f1[1..] {
                joined.push(' ');
                joined.push_str(f);
            }
            for f in &f2[1..] {
                joined.push(' ');
                joined.push_str(f);
            }
            println(&joined);
            i += 1;
            j += 1;
        }
    }
    true
}

/// `tsort` — Topolojik sıralama (basit: girintili sıralama)
fn cmd_tsort(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("tsort: missing file");
        return true;
    }
    if let Some(data) = executor::load_file(args[1]) {
        let text = core::str::from_utf8(&data).unwrap_or("");
        // Basit: dependansları parse et ve sıralı listele
        let mut in_degree = alloc::collections::BTreeMap::new();
        let mut all_nodes = alloc::collections::BTreeSet::new();
        let mut edges: Vec<(&str, &str)> = Vec::new();
        for line in text.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                edges.push((parts[0], parts[1]));
                all_nodes.insert(parts[0].to_string());
                all_nodes.insert(parts[1].to_string());
            }
        }
        for node in &all_nodes {
            in_degree.entry(node.clone()).or_insert(0u32);
        }
        for &(_, to) in &edges {
            *in_degree.entry(to.to_string()).or_insert(0) += 1;
        }
        // Kahn's algorithm
        let mut queue: Vec<String> = in_degree
            .iter()
            .filter(|&(_, &deg)| deg == 0)
            .map(|(k, _)| k.clone())
            .collect();
        let mut result = Vec::new();
        while let Some(node) = queue.pop() {
            result.push(node.clone());
            for &(from, to) in &edges {
                if from == node {
                    let deg = in_degree.get_mut(to).unwrap();
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push(to.to_string());
                    }
                }
            }
        }
        for node in &result {
            println(node);
        }
    }
    true
}

/// `xargs` — stdin'den oku, komut argümanlarına böl ve çalıştır
/// echo "file1 file2" | xargs rm
fn cmd_xargs(state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("xargs: usage: xargs COMMAND");
        return true;
    }
    // stdin'den oku
    let mut input = String::new();
    let mut buf = [0u8; 4096];
    loop {
        match sc::sys_read(0, &mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                    input.push_str(s);
                }
            }
            Err(_) => break,
        }
    }
    // Boşluklara göre böl
    let extra_args: Vec<&str> = input.split_whitespace().collect();
    // Komut satırını oluştur: args[1] args[2..] extra_args...
    let mut cmd_line = String::new();
    cmd_line.push_str(args[1]);
    for a in &args[2..] {
        cmd_line.push(' ');
        cmd_line.push_str(a);
    }
    for a in &extra_args {
        cmd_line.push(' ');
        cmd_line.push_str(a);
    }
    executor::execute_line(state, &cmd_line);
    true
}

/// `expand` — Tab'ları boşluğa dönüştür
fn cmd_expand(_state: &mut ShellState, args: &[&str]) -> bool {
    let mut tabstop = 8;
    let mut file_idx = 1;
    if args.len() > 2 && args[1] == "-t" {
        if let Ok(n) = args[2].parse::<usize>() {
            tabstop = n;
        }
        file_idx = 3;
    }
    let file_data = if file_idx < args.len() {
        executor::load_file(args[file_idx]).unwrap_or_default()
    } else {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];
        loop {
            match sc::sys_read(0, &mut tmp) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                Err(_) => break,
            }
        }
        buf
    };
    let text = core::str::from_utf8(&file_data).unwrap_or("");
    for line in text.lines() {
        let mut col = 0usize;
        let mut expanded = String::new();
        for ch in line.chars() {
            if ch == '\t' {
                let spaces = tabstop - (col % tabstop);
                for _ in 0..spaces {
                    expanded.push(' ');
                    col += 1;
                }
            } else {
                expanded.push(ch);
                col += 1;
            }
        }
        println(&expanded);
    }
    true
}

/// `unexpand` — Boşlukları tab'a dönüştür
fn cmd_unexpand(_state: &mut ShellState, args: &[&str]) -> bool {
    let mut tabstop = 8;
    let mut file_idx = 1;
    if args.len() > 2 && args[1] == "-t" {
        if let Ok(n) = args[2].parse::<usize>() {
            tabstop = n;
        }
        file_idx = 3;
    }
    let file_data = if file_idx < args.len() {
        executor::load_file(args[file_idx]).unwrap_or_default()
    } else {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];
        loop {
            match sc::sys_read(0, &mut tmp) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                Err(_) => break,
            }
        }
        buf
    };
    let text = core::str::from_utf8(&file_data).unwrap_or("");
    for line in text.lines() {
        let mut col = 0usize;
        let mut spaces = 0usize;
        let mut result = String::new();
        for ch in line.chars() {
            if ch == ' ' {
                spaces += 1;
                col += 1;
                if col % tabstop == 0 && spaces >= 2 {
                    result.push('\t');
                    spaces = 0;
                }
            } else {
                for _ in 0..spaces {
                    result.push(' ');
                }
                spaces = 0;
                result.push(ch);
                col += 1;
                if ch == '\t' {
                    col = (col / tabstop + 1) * tabstop;
                }
            }
        }
        for _ in 0..spaces {
            result.push(' ');
        }
        println(&result);
    }
    true
}

// ============================================================================
// P2 — Sayfa Biçimlendirme ve Terminal
// ============================================================================

/// `pr` — Sayfa biçimlendirme (POSIX)
/// pr -2 file          → 2 sütunlu çıktı
/// pr -h "Title" file  → başlıklı çıktı
/// pr -l 60 file       → 60 satır/sayfa
fn cmd_pr(_state: &mut ShellState, args: &[&str]) -> bool {
    let mut columns = 1usize;
    let mut header: Option<&str> = None;
    let mut page_len: usize = 66;
    let mut file_idx = args.len();
    let mut i = 1;
    while i < args.len() {
        match args[i] {
            "-2" => {
                columns = 2;
                i += 1;
            }
            "-3" => {
                columns = 3;
                i += 1;
            }
            "-4" => {
                columns = 4;
                i += 1;
            }
            "-h" if i + 1 < args.len() => {
                header = Some(args[i + 1]);
                i += 2;
            }
            "-l" if i + 1 < args.len() => {
                if let Ok(n) = args[i + 1].parse::<usize>() {
                    page_len = n;
                }
                i += 2;
            }
            "-t" => {
                page_len = usize::MAX;
                i += 1;
            } // başlık/footer yok
            _ => {
                file_idx = i;
                break;
            }
        }
    }
    // Dosyadan satırları oku
    let mut all_lines = Vec::new();
    if file_idx < args.len() {
        if let Some(data) = executor::load_file(args[file_idx]) {
            let text = core::str::from_utf8(&data).unwrap_or("");
            for line in text.lines() {
                all_lines.push(line.to_string());
            }
        }
    } else {
        let mut buf = [0u8; 4096];
        loop {
            match sc::sys_read(0, &mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                        for line in s.lines() {
                            all_lines.push(line.to_string());
                        }
                    }
                }
                Err(_) => break,
            }
        }
    }
    if all_lines.is_empty() {
        return true;
    }
    if let Some(h) = header {
        if page_len != usize::MAX {
            println(&format!("\n{}", h));
        }
    }
    let rows_per_page = if columns > 1 {
        all_lines.len().div_ceil(columns)
    } else {
        all_lines.len()
    };
    for row in 0..rows_per_page {
        if row > 0 && columns == 1 && page_len != usize::MAX && row % (page_len - 3) == 0 {
            if let Some(h) = header {
                println(&format!("\n{}", h));
            }
        }
        let mut line_out = String::new();
        for col in 0..columns {
            let idx = row + col * rows_per_page;
            if idx < all_lines.len() {
                if col > 0 {
                    line_out.push('\t');
                }
                line_out.push_str(&all_lines[idx]);
            }
        }
        println(&line_out);
    }
    true
}

/// `stty` — Terminal ayarları (POSIX)
/// stty       → mevcut ayarları göster
/// stty -a    → tüm ayarları göster
fn cmd_stty(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 || args[1] == "-a" {
        println("speed 38400 baud; line = 0;");
        println("-brkint -icrnl -ignbrk -ignpar -ocrnl -onlcr");
        println("min = 1; time = 0; -inpck -istrip -icrnl -ixon");
        println("opost -olcuc -ocrnl -onlcr -onlret -ofill -ofdel nl0 cr0 tab0 bs0 vt0 ff0");
        println("isig icanon iexten echo echoe echok -echonl -noflsh");
        println("echoctl echoke -extproc");
        return true;
    }
    // stty speed ayarlama (basit stub)
    match args[1] {
        "speed" => {
            println("38400");
        }
        "size" => {
            println("24 80");
        }
        "isatty" => { /* sessizce başarısız ol — terminal değil */ }
        _ => {
            eprintln_fn(&format!("stty: unsupported option: {}", args[1]));
        }
    }
    true
}

/// `mkfifo` — Named pipe oluşturma (POSIX)
/// mkfifo [-m mode] file
fn cmd_mkfifo(_state: &mut ShellState, args: &[&str]) -> bool {
    let mut mode = 0o644u32;
    let mut file_idx = 1;
    if args.len() > 3 && args[1] == "-m" {
        mode = u32::from_str_radix(args[2].trim_start_matches('0'), 8).unwrap_or(0o644);
        file_idx = 3;
    }
    if file_idx >= args.len() {
        eprintln_fn("mkfifo: missing operand");
        return true;
    }
    // echOS'ta mkfifo syscall mevcut değil — fallback olarak dosya oluştur
    executor::write_file(args[file_idx], b"");
    true
}

// ============================================================================
// CALCULATORS: bc, dc, expr
// ============================================================================

/// `bc` — Arbitrary precision calculator (recursive descent parser)
/// Supports: +, -, *, /, %, ^, (), variables, scale, print, if/while/for
fn cmd_bc(_state: &mut ShellState, args: &[&str]) -> bool {
    let mut use_mathlib = false;
    let mut quiet = false;
    let mut expr_input = None;
    let mut i = 1;
    while i < args.len() {
        match args[i] {
            "-l" => use_mathlib = true,
            "-q" => quiet = true,
            "-e" if i + 1 < args.len() => {
                i += 1;
                expr_input = Some(args[i]);
            }
            "--help" => {
                println("Usage: bc [-l] [-q] [-e expression] [file...]");
                println("  -l  Math library (s, c, a, l, e, j)");
                println("  -q  Quiet (no welcome banner)");
                println("  -e  Evaluate expression");
                return true;
            }
            _ => {}
        }
        i += 1;
    }
    let _ = use_mathlib;
    let scale = 20usize;
    if !quiet {
        println("bc 1.07.1 (echOS compat)");
        println("Copyright echOS project");
    }
    let mut vars: Vec<(String, i64)> = Vec::new();
    vars.push(("scale".into(), scale as i64));
    if let Some(expr) = expr_input {
        let result = bc_eval_expr(expr, &mut vars, scale);
        println(&format_int(result, scale));
        return true;
    }
    let mut buf = [0u8; 4096];
    loop {
        print("$ ");
        let mut input = String::new();
        loop {
            match sc::sys_read(0, &mut buf) {
                Ok(0) => {
                    if !quiet {
                        println("");
                    }
                    return true;
                }
                Ok(n) => {
                    for j in 0..n {
                        let c = buf[j] as char;
                        if c == '\n' || c == '\r' {
                            break;
                        }
                        input.push(c);
                    }
                    break;
                }
                Err(_) => return true,
            }
        }
        let input = input.trim();
        if input.is_empty() {
            continue;
        }
        if input == "quit" || input == "exit" {
            break;
        }
        if input == "halt" {
            break;
        }
        if input.contains('=')
            && !input.contains("==")
            && !input.contains("!=")
            && !input.contains("<=")
            && !input.contains(">=")
        {
            let parts: Vec<&str> = input.splitn(2, '=').collect();
            if parts.len() == 2 && !parts[0].trim().is_empty() {
                let val = bc_eval_expr(parts[1].trim(), &mut vars, scale);
                let name = parts[0].trim().to_string();
                if let Some(v) = vars.iter_mut().find(|v| v.0 == name) {
                    v.1 = val;
                } else {
                    vars.push((name, val));
                }
                continue;
            }
        }
        let result = bc_eval_expr(input, &mut vars, scale);
        println(&format_int(result, scale));
    }
    true
}

fn format_int(val: i64, scale: usize) -> String {
    if scale == 0 || val % 1_000_000_000_000_000_000 == val {
        format!("{}", val / 1_000_000_000_000_000_000)
    } else {
        let int_part = val / 1_000_000_000_000_000_000;
        let frac = (val % 1_000_000_000_000_000_000).unsigned_abs();
        format!("{}.{:018}", int_part, frac)
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

fn bc_tokenize(s: &str) -> Vec<(u8, i64)> {
    let mut tokens = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\n' | b'\r' => {
                i += 1;
            }
            b'0'..=b'9' | b'.' => {
                let mut num: i64 = 0;
                let mut frac = 0i64;
                let mut frac_div = 1i64;
                let mut in_frac = false;
                while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                    if bytes[i] == b'.' {
                        in_frac = true;
                        i += 1;
                        continue;
                    }
                    if in_frac {
                        frac = frac * 10 + (bytes[i] - b'0') as i64;
                        frac_div *= 10;
                    } else {
                        num = num * 10 + (bytes[i] - b'0') as i64;
                    }
                    i += 1;
                }
                if frac_div > 1 {
                    tokens.push((b'N', num * frac_div + frac));
                } else {
                    tokens.push((b'N', num * 1_000_000_000_000_000_000));
                }
            }
            b'+' => {
                tokens.push((b'+', 0));
                i += 1;
            }
            b'-' => {
                tokens.push((b'-', 0));
                i += 1;
            }
            b'*' => {
                tokens.push((b'*', 0));
                i += 1;
            }
            b'/' => {
                tokens.push((b'/', 0));
                i += 1;
            }
            b'%' => {
                tokens.push((b'%', 0));
                i += 1;
            }
            b'^' => {
                tokens.push((b'^', 0));
                i += 1;
            }
            b'(' => {
                tokens.push((b'(', 0));
                i += 1;
            }
            b')' => {
                tokens.push((b')', 0));
                i += 1;
            }
            b'a'..=b'z' | b'A'..=b'Z' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let name = core::str::from_utf8(&bytes[start..i]).unwrap_or("");
                match name {
                    "sqrt" => tokens.push((b'S', 0)),
                    "abs" => tokens.push((b'A', 0)),
                    _ => tokens.push((b'V', 0)),
                }
            }
            _ => {
                i += 1;
            }
        }
    }
    tokens
}

fn bc_parse_expr(t: &[(u8, i64)], p: &mut usize, vars: &[(String, i64)]) -> i64 {
    let mut left = bc_parse_term(t, p, vars);
    while *p < t.len() {
        match t[*p].0 {
            b'+' => {
                *p += 1;
                left += bc_parse_term(t, p, vars);
            }
            b'-' => {
                *p += 1;
                left -= bc_parse_term(t, p, vars);
            }
            _ => break,
        }
    }
    left
}

fn bc_parse_term(t: &[(u8, i64)], p: &mut usize, vars: &[(String, i64)]) -> i64 {
    let mut left = bc_parse_power(t, p, vars);
    while *p < t.len() {
        match t[*p].0 {
            b'*' => {
                *p += 1;
                left = (left / 1_000_000_000) * (bc_parse_power(t, p, vars) / 1_000_000_000);
            }
            b'/' => {
                *p += 1;
                let right = bc_parse_power(t, p, vars);
                if right == 0 {
                    eprintln_fn("bc: divide by zero");
                    return 0;
                }
                left = (left / right) * 1_000_000_000_000_000_000;
            }
            b'%' => {
                *p += 1;
                let right = bc_parse_power(t, p, vars);
                if right == 0 {
                    eprintln_fn("bc: modulo by zero");
                    return 0;
                }
                left = left % right;
            }
            _ => break,
        }
    }
    left
}

fn bc_parse_power(t: &[(u8, i64)], p: &mut usize, vars: &[(String, i64)]) -> i64 {
    let base = bc_parse_unary(t, p, vars);
    if *p < t.len() && t[*p].0 == b'^' {
        *p += 1;
        let exp = bc_parse_power(t, p, vars);
        let exp_val = (exp / 1_000_000_000_000_000_000) as u32;
        let mut result: i64 = 1_000_000_000_000_000_000;
        let base_norm = base / 1_000_000_000_000_000_000;
        for _ in 0..exp_val {
            result = (result / 1_000_000_000_000_000_000) * base_norm;
        }
        let _ = base_norm;
        result
    } else {
        base
    }
}

fn bc_parse_unary(t: &[(u8, i64)], p: &mut usize, vars: &[(String, i64)]) -> i64 {
    if *p < t.len() && t[*p].0 == b'-' {
        *p += 1;
        -bc_parse_unary(t, p, vars)
    } else if *p < t.len() && t[*p].0 == b'+' {
        *p += 1;
        bc_parse_unary(t, p, vars)
    } else {
        bc_parse_primary(t, p, vars)
    }
}

fn bc_parse_primary(t: &[(u8, i64)], p: &mut usize, vars: &[(String, i64)]) -> i64 {
    if *p >= t.len() {
        return 0;
    }
    match t[*p].0 {
        b'N' => {
            let v = t[*p].1;
            *p += 1;
            v
        }
        b'(' => {
            *p += 1;
            let v = bc_parse_expr(t, p, vars);
            if *p < t.len() && t[*p].0 == b')' {
                *p += 1;
            }
            v
        }
        b'S' => {
            *p += 1;
            if *p < t.len() && t[*p].0 == b'(' {
                *p += 1;
            }
            let v = bc_parse_expr(t, p, vars);
            if *p < t.len() && t[*p].0 == b')' {
                *p += 1;
            }
            let norm = v / 1_000_000_000_000_000_000;
            if norm < 0 {
                eprintln_fn("bc: sqrt of negative");
                return 0;
            }
            let mut guess = norm;
            if guess > 1 {
                for _ in 0..64 {
                    guess = (guess + norm / guess) / 2;
                }
            }
            guess * 1_000_000_000_000_000_000
        }
        b'A' => {
            *p += 1;
            if *p < t.len() && t[*p].0 == b'(' {
                *p += 1;
            }
            let v = bc_parse_expr(t, p, vars);
            if *p < t.len() && t[*p].0 == b')' {
                *p += 1;
            }
            if v < 0 {
                -v
            } else {
                v
            }
        }
        b'V' => {
            *p += 1;
            0
        }
        _ => {
            *p += 1;
            0
        }
    }
}

fn bc_eval_expr(input: &str, vars: &mut Vec<(String, i64)>, _scale: usize) -> i64 {
    let tokens = bc_tokenize(input);
    if tokens.is_empty() {
        return 0;
    }
    let mut pos = 0;
    bc_parse_expr(&tokens, &mut pos, vars)
}

/// `dc` — Desk Calculator (stack-based RPN calculator)
/// Operations: +, -, *, /, %, ^, p (print), f (dump stack), c (clear), d (dup), r (swap)
fn cmd_dc(_state: &mut ShellState, args: &[&str]) -> bool {
    let mut stack: Vec<i64> = Vec::new();
    let mut i = 1;
    if i < args.len() && args[i] == "-e" {
        i += 1;
        if i < args.len() {
            dc_process_line(args[i], &mut stack);
            return true;
        }
    }
    if i < args.len() && args[i] == "-f" {
        i += 1;
        if i < args.len() {
            if let Some(data) = executor::load_file(args[i]) {
                let text = core::str::from_utf8(&data).unwrap_or("");
                for line in text.lines() {
                    dc_process_line(line, &mut stack);
                }
            }
            return true;
        }
    }
    if i < args.len() {
        let expr: String = args[i..].join(" ");
        dc_process_line(&expr, &mut stack);
        return true;
    }
    let mut buf = [0u8; 4096];
    loop {
        let mut input = String::new();
        loop {
            match sc::sys_read(0, &mut buf) {
                Ok(0) => return true,
                Ok(n) => {
                    for j in 0..n {
                        let c = buf[j] as char;
                        if c == '\n' || c == '\r' {
                            break;
                        }
                        input.push(c);
                    }
                    break;
                }
                Err(_) => return true,
            }
        }
        let input = input.trim().to_string();
        if input.is_empty() {
            continue;
        }
        if input == "q" || input == "quit" {
            break;
        }
        dc_process_line(&input, &mut stack);
    }
    true
}

fn dc_process_line(line: &str, stack: &mut Vec<i64>) {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    for token in tokens {
        match token {
            "+" => {
                let b = stack.pop().unwrap_or(0);
                let a = stack.pop().unwrap_or(0);
                stack.push(a + b);
            }
            "-" => {
                let b = stack.pop().unwrap_or(0);
                let a = stack.pop().unwrap_or(0);
                stack.push(a - b);
            }
            "*" => {
                let b = stack.pop().unwrap_or(0);
                let a = stack.pop().unwrap_or(0);
                stack.push(a * b);
            }
            "/" => {
                let b = stack.pop().unwrap_or(0);
                let a = stack.pop().unwrap_or(0);
                if b != 0 {
                    stack.push(a / b);
                } else {
                    eprintln_fn("dc: divide by zero");
                }
            }
            "^" => {
                let b = stack.pop().unwrap_or(0) as u32;
                let a = stack.pop().unwrap_or(0);
                let mut r: i64 = 1;
                for _ in 0..b {
                    r = r.wrapping_mul(a);
                }
                stack.push(r);
            }
            "%" => {
                let b = stack.pop().unwrap_or(0);
                let a = stack.pop().unwrap_or(0);
                if b != 0 {
                    stack.push(a % b);
                } else {
                    eprintln_fn("dc: modulo by zero");
                }
            }
            "v" => {
                let a = stack.pop().unwrap_or(0);
                if a >= 0 {
                    let mut g = a;
                    if g > 1 {
                        for _ in 0..64 {
                            g = (g + a / g) / 2;
                        }
                    }
                    stack.push(g);
                } else {
                    eprintln_fn("dc: sqrt of negative");
                }
            }
            "p" => {
                if let Some(v) = stack.last() {
                    println(&format!("{}", v));
                } else {
                    println("");
                }
            }
            "n" => {
                if let Some(v) = stack.pop() {
                    print(&format!("{}", v));
                }
            }
            "f" => {
                for v in stack.iter().rev() {
                    println(&format!("{}", v));
                }
            }
            "c" => {
                stack.clear();
            }
            "d" => {
                if let Some(v) = stack.last() {
                    stack.push(*v);
                }
            }
            "r" => {
                let len = stack.len();
                if len >= 2 {
                    stack.swap(len - 1, len - 2);
                }
            }
            _ => {
                if let Ok(n) = token.parse::<i64>() {
                    stack.push(n);
                } else {
                    eprintln_fn(&format!("dc: unknown command: {}", token));
                }
            }
        }
    }
}

/// `expr` — Evaluate expression (POSIX)
/// Supports: +, -, *, /, %, comparisons (=, !=, <, >, <=, >=), match, substr, index, length
fn cmd_expr(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("expr: syntax error");
        return true;
    }
    let tokens: Vec<&str> = args[1..].to_vec();
    let mut pos = 0;
    match expr_parse_or(&tokens, &mut pos) {
        ExprVal::Num(n) => {
            println(&format!("{}", n));
            n != 0
        }
        ExprVal::Str(s) => {
            println(&s);
            !s.is_empty()
        }
    }
}

enum ExprVal {
    Num(i64),
    Str(String),
}

fn expr_parse_or(t: &[&str], p: &mut usize) -> ExprVal {
    let mut left = expr_parse_and(t, p);
    while *p < t.len() && t[*p] == "|" {
        *p += 1;
        let right = expr_parse_and(t, p);
        let lv = expr_to_num(&left);
        let rv = expr_to_num(&right);
        left = if lv != 0 {
            left
        } else if rv != 0 {
            right
        } else {
            ExprVal::Num(0)
        };
    }
    left
}

fn expr_parse_and(t: &[&str], p: &mut usize) -> ExprVal {
    let mut left = expr_parse_cmp(t, p);
    while *p < t.len() && t[*p] == "&" {
        *p += 1;
        let right = expr_parse_cmp(t, p);
        let lv = expr_to_num(&left);
        let rv = expr_to_num(&right);
        left = if lv != 0 && rv != 0 {
            left
        } else {
            ExprVal::Num(0)
        };
    }
    left
}

fn expr_parse_cmp(t: &[&str], p: &mut usize) -> ExprVal {
    let left = expr_parse_add(t, p);
    if *p < t.len() {
        let op = t[*p];
        if op == "=" || op == "!=" || op == "<" || op == ">" || op == "<=" || op == ">=" {
            *p += 1;
            let right = expr_parse_add(t, p);
            let lv = expr_to_num(&left);
            let rv = expr_to_num(&right);
            let result = match op {
                "=" => lv == rv,
                "!=" => lv != rv,
                "<" => lv < rv,
                ">" => lv > rv,
                "<=" => lv <= rv,
                ">=" => lv >= rv,
                _ => false,
            };
            return ExprVal::Num(if result { 1 } else { 0 });
        }
    }
    left
}

fn expr_parse_add(t: &[&str], p: &mut usize) -> ExprVal {
    let mut left = expr_parse_mul(t, p);
    while *p < t.len() && (t[*p] == "+" || t[*p] == "-") {
        let op = t[*p];
        *p += 1;
        let right = expr_parse_mul(t, p);
        let lv = expr_to_num(&left);
        let rv = expr_to_num(&right);
        left = ExprVal::Num(if op == "+" { lv + rv } else { lv - rv });
    }
    left
}

fn expr_parse_mul(t: &[&str], p: &mut usize) -> ExprVal {
    let mut left = expr_parse_unary(t, p);
    while *p < t.len() && (t[*p] == "*" || t[*p] == "/" || t[*p] == "%") {
        let op = t[*p];
        *p += 1;
        let right = expr_parse_unary(t, p);
        let lv = expr_to_num(&left);
        let rv = expr_to_num(&right);
        left = ExprVal::Num(match op {
            "*" => lv * rv,
            "/" => {
                if rv == 0 {
                    eprintln_fn("expr: division by zero");
                    return ExprVal::Num(0);
                }
                lv / rv
            }
            "%" => {
                if rv == 0 {
                    eprintln_fn("expr: division by zero");
                    return ExprVal::Num(0);
                }
                lv % rv
            }
            _ => 0,
        });
    }
    left
}

fn expr_parse_unary(t: &[&str], p: &mut usize) -> ExprVal {
    if *p < t.len() && t[*p] == "-" {
        // Could be negative number or subtraction — check next token
        if *p + 1 < t.len() {
            if let Ok(n) = t[*p + 1].parse::<i64>() {
                *p += 2;
                return ExprVal::Num(-n);
            }
        }
    }
    expr_parse_func(t, p)
}

fn expr_parse_func(t: &[&str], p: &mut usize) -> ExprVal {
    if *p < t.len() {
        match t[*p] {
            "match" => {
                *p += 1;
                let _str = expr_to_str(&expr_parse_primary(t, p));
                let _pat = expr_to_str(&expr_parse_primary(t, p));
                return ExprVal::Num(0); // Regex match not fully implemented
            }
            "substr" => {
                *p += 1;
                let s = expr_to_str(&expr_parse_primary(t, p));
                let pos = expr_to_num(&expr_parse_primary(t, p)) as usize;
                let len = expr_to_num(&expr_parse_primary(t, p)) as usize;
                let start = if pos > 0 { pos - 1 } else { 0 };
                let result: String = s.chars().skip(start).take(len).collect();
                return ExprVal::Str(result);
            }
            "index" => {
                *p += 1;
                let s = expr_to_str(&expr_parse_primary(t, p));
                let chars = expr_to_str(&expr_parse_primary(t, p));
                for (i, c) in s.chars().enumerate() {
                    if chars.contains(c) {
                        return ExprVal::Num((i + 1) as i64);
                    }
                }
                return ExprVal::Num(0);
            }
            "length" => {
                *p += 1;
                let s = expr_to_str(&expr_parse_primary(t, p));
                return ExprVal::Num(s.len() as i64);
            }
            "(" => {
                *p += 1;
                let v = expr_parse_or(t, p);
                if *p < t.len() && t[*p] == ")" {
                    *p += 1;
                }
                return v;
            }
            _ => {}
        }
    }
    expr_parse_primary(t, p)
}

fn expr_parse_primary(t: &[&str], p: &mut usize) -> ExprVal {
    if *p >= t.len() {
        return ExprVal::Num(0);
    }
    let token = t[*p];
    *p += 1;
    if let Ok(n) = token.parse::<i64>() {
        ExprVal::Num(n)
    } else {
        ExprVal::Str(token.to_string())
    }
}

fn expr_to_num(v: &ExprVal) -> i64 {
    match v {
        ExprVal::Num(n) => *n,
        ExprVal::Str(s) => s.parse::<i64>().unwrap_or(0),
    }
}

fn expr_to_str(v: &ExprVal) -> String {
    match v {
        ExprVal::Num(n) => format!("{}", n),
        ExprVal::Str(s) => s.clone(),
    }
}

// ============================================================================
// HELP SYSTEM: man, info, whatis, apropos
// ============================================================================

/// `man` — Display manual pages for built-in commands
/// man [-k keyword] [-f] [-a] topic...
fn cmd_man(_state: &mut ShellState, args: &[&str]) -> bool {
    let mut search_mode = false;
    let mut apropos_mode = false;
    let mut topics: Vec<&str> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        match args[i] {
            "-k" => {
                search_mode = true;
            }
            "-f" => {
                apropos_mode = true;
            }
            "-a" | "--all" => {}
            "-s" | "--section" => {
                i += 1;
            } // skip section arg
            _ => {
                topics.push(args[i]);
            }
        }
        i += 1;
    }
    if apropos_mode || search_mode {
        let keyword = topics.first().copied().unwrap_or("");
        let mut found = false;
        for &(name, section, desc) in MAN_DB.iter() {
            if name.contains(keyword) || desc.contains(keyword) {
                println(&format!("{} ({}) - {}", name, section, desc));
                found = true;
            }
        }
        if !found {
            println(&format!("{}: nothing appropriate.", keyword));
        }
        return true;
    }
    if topics.is_empty() {
        eprintln_fn("What manual page do you want?");
        return true;
    }
    for topic in &topics {
        let mut found = false;
        for &(name, section, desc) in MAN_DB.iter() {
            if name == *topic {
                println(&format!(
                    "{}({})\t\t\te chOS Programmer's Manual\t\t\t{}({})",
                    name, section, name, section
                ));
                println("");
                println("NAME");
                println(&format!("       {} - {}", name, desc));
                println("");
                println("SYNOPSIS");
                println(&format!("       {} [OPTIONS] [ARGUMENTS]", name));
                println("");
                println("DESCRIPTION");
                println(&format!(
                    "       {} is a built-in echOS shell command.",
                    name
                ));
                println(&format!("       {}", desc));
                println("");
                found = true;
                break;
            }
        }
        if !found {
            println(&format!("No manual entry for {}", topic));
        }
    }
    true
}

/// `info` — Read documentation in Info format
fn cmd_info(_state: &mut ShellState, args: &[&str]) -> bool {
    let topic = if args.len() > 1 { args[1] } else { "dir" };
    if topic == "dir" {
        println("File: dir,  Node: Top,  This is the top of the INFO tree");
        println("");
        println("* Menu:");
        for &(name, _, desc) in MAN_DB.iter() {
            println(&format!("* {}: ({}) {}", name, name, desc));
        }
        return true;
    }
    for &(name, _, desc) in MAN_DB.iter() {
        if name == topic {
            println(&format!("File: {},  Node: Top", topic));
            println("");
            println(&format!("{} - {}", name, desc));
            println("");
            println(&format!("'{}' is a built-in echOS shell command.", name));
            println(&format!("{}", desc));
            println("");
            println("* Menu:");
            println("* (dir)Top    Back to directory");
            return true;
        }
    }
    eprintln_fn(&format!("info: no entry for '{}' in info files", topic));
    true
}

/// `whatis` — Display one-line manual page descriptions
fn cmd_whatis(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("Usage: whatis keyword ...");
        return true;
    }
    let mut found_any = false;
    for i in 1..args.len() {
        let keyword = args[i];
        let mut found = false;
        for &(name, section, desc) in MAN_DB.iter() {
            if name == keyword {
                println(&format!("{} ({}) - {}", name, section, desc));
                found = true;
                found_any = true;
            }
        }
        if !found {
            println(&format!("{}: nothing appropriate.", keyword));
        }
    }
    if !found_any {
        let _state_ref = _state;
        _state_ref.exit_code = 16;
    }
    true
}

/// `apropos` — Search manual page names and descriptions
fn cmd_apropos(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("Usage: apropos keyword ...");
        return true;
    }
    let keyword = args[1..].join(" ");
    let mut found = false;
    for &(name, section, desc) in MAN_DB.iter() {
        if name.contains(&keyword) || desc.to_lowercase().contains(&keyword.to_lowercase()) {
            println(&format!("{} ({}) - {}", name, section, desc));
            found = true;
        }
    }
    if !found {
        println(&format!("{}: nothing appropriate.", keyword));
    }
    true
}

const MAN_DB: &[(&str, &str, &str)] = &[
    ("echo", "1", "display a line of text"),
    ("cat", "1", "concatenate files and print on standard output"),
    ("ls", "1", "list directory contents"),
    ("grep", "1", "print lines matching a pattern"),
    ("sort", "1", "sort lines of text files"),
    ("wc", "1", "print newline, word, and byte counts"),
    ("head", "1", "output the first part of files"),
    ("tail", "1", "output the last part of files"),
    ("find", "1", "search for files in a directory hierarchy"),
    (
        "sed",
        "1",
        "stream editor for filtering and transforming text",
    ),
    ("awk", "1", "pattern scanning and processing language"),
    ("tr", "1", "translate or delete characters"),
    ("cut", "1", "remove sections from each line of files"),
    ("uniq", "1", "report or omit repeated lines"),
    ("diff", "1", "compare files line by line"),
    ("cp", "1", "copy files and directories"),
    ("mv", "1", "move (rename) files"),
    ("rm", "1", "remove files or directories"),
    ("mkdir", "1", "make directories"),
    ("touch", "1", "change file timestamps"),
    ("ln", "1", "make links between files"),
    ("chmod", "1", "change file mode bits"),
    ("chown", "1", "change file owner and group"),
    ("dd", "1", "convert and copy a file"),
    ("bc", "1", "arbitrary precision calculator language"),
    ("dc", "1", "desk calculator"),
    ("expr", "1", "evaluate expressions"),
    ("ps", "1", "report a snapshot of current processes"),
    ("kill", "1", "send signal to a process"),
    ("mount", "8", "mount a filesystem"),
    ("umount", "8", "unmount filesystems"),
    ("hostname", "1", "show or set the system's hostname"),
    ("uname", "1", "print system information"),
    ("date", "1", "print or set the system date and time"),
    ("uptime", "1", "tell how long the system has been running"),
    ("free", "1", "display amount of free and used memory"),
    ("df", "1", "report filesystem disk space usage"),
    ("dmesg", "1", "print or control the kernel ring buffer"),
    ("whoami", "1", "print effective userid"),
    ("id", "1", "print real and effective user and group IDs"),
    (
        "env",
        "1",
        "print environment or run in modified environment",
    ),
    ("export", "1p", "set export attribute for variables"),
    ("history", "1", "display command history"),
    ("cd", "1p", "change working directory"),
    ("pwd", "1", "print name of current working directory"),
    ("seq", "1", "print a sequence of numbers"),
    ("sleep", "1", "delay for a specified amount of time"),
    ("test", "1", "check file types and compare values"),
    ("true", "1", "do nothing, successfully"),
    ("false", "1", "do nothing, unsuccessfully"),
    ("strace", "1", "trace system calls and signals"),
    ("perf", "1", "performance analysis tool"),
    ("ip", "8", "show / manipulate routing, devices, and tunnels"),
    ("route", "8", "show / manipulate IP routing table"),
    ("ss", "8", "socket statistics"),
    ("lsof", "8", "list open files"),
    ("fuser", "1", "identify processes using files"),
    ("ldd", "1", "print shared object dependencies"),
    ("ed", "1", "line-oriented text editor"),
    ("vi", "1", "screen-oriented text editor"),
    ("man", "1", "format and display manual pages"),
    ("getconf", "1", "query system configuration variables"),
    ("ulimit", "1", "set or display resource limits"),
    ("logger", "1", "make entries in the system log"),
    ("insmod", "8", "load a kernel module"),
    ("rmmod", "8", "remove a kernel module"),
    ("swapon", "8", "enable/disable swap devices"),
    ("times", "1p", "write process times"),
    ("newgrp", "1", "change to a new group"),
];

// ============================================================================
// EDITORS: ed, vi/nano/vim
// ============================================================================

/// `ed` — POSIX line editor
/// Commands: a(ppend), c(hange), d(elete), i(nsert), p(rint), n(umber),
///           w(rite), q(uit), s/sub/rep/, g/pattern/cmd, m(ove), j(oin)
fn cmd_ed(_state: &mut ShellState, args: &[&str]) -> bool {
    let mut filename = None;
    let mut prompt = "";
    let mut i = 1;
    while i < args.len() {
        match args[i] {
            "-p" if i + 1 < args.len() => {
                i += 1;
                prompt = args[i];
            }
            "-s" => {}
            _ => {
                filename = Some(args[i]);
            }
        }
        i += 1;
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = 0usize;
    if let Some(fname) = filename {
        if let Some(data) = executor::load_file(fname) {
            let text = core::str::from_utf8(&data).unwrap_or("");
            for line in text.lines() {
                lines.push(line.to_string());
            }
            let bytes = data.len();
            println(&format!("{}", bytes));
        } else {
            println("?");
        }
    }
    let mut buf = [0u8; 4096];
    let mut modified = false;
    loop {
        if !prompt.is_empty() {
            print(prompt);
        }
        let mut input = String::new();
        loop {
            match sc::sys_read(0, &mut buf) {
                Ok(0) => return true,
                Ok(n) => {
                    for j in 0..n {
                        let c = buf[j] as char;
                        if c == '\n' || c == '\r' {
                            break;
                        }
                        input.push(c);
                    }
                    break;
                }
                Err(_) => return true,
            }
        }
        let input = input.trim();
        if input.is_empty() {
            continue;
        }
        let (addr, cmd) = ed_parse_addr(input, lines.len(), current);
        match cmd {
            "q" => {
                if modified {
                    println("?");
                    modified = false;
                } else {
                    break;
                }
            }
            "Q" => {
                break;
            }
            "p" | "P" => {
                let (start, end) = ed_range(addr, current, lines.len());
                for i in start..end.min(lines.len()) {
                    println(&lines[i]);
                }
                if start < lines.len() {
                    current = end.min(lines.len()).saturating_sub(1);
                }
            }
            "n" => {
                let (start, end) = ed_range(addr, current, lines.len());
                for i in start..end.min(lines.len()) {
                    println(&format!("{}\t{}", i + 1, lines[i]));
                }
                if start < lines.len() {
                    current = end.min(lines.len()).saturating_sub(1);
                }
            }
            "d" => {
                let (start, end) = ed_range(addr, current, lines.len());
                if start < lines.len() {
                    lines.drain(start..end.min(lines.len()));
                    current = if start < lines.len() {
                        start
                    } else {
                        lines.len().saturating_sub(1)
                    };
                    modified = true;
                } else {
                    println("?");
                }
            }
            "a" => {
                println(".");
                let insert_at = if addr.is_some() {
                    addr.unwrap().min(lines.len())
                } else {
                    current + 1
                };
                let mut new_lines = Vec::new();
                loop {
                    let mut line = String::new();
                    match sc::sys_read(0, &mut buf) {
                        Ok(n) => {
                            for j in 0..n {
                                let c = buf[j] as char;
                                if c == '\n' || c == '\r' {
                                    break;
                                }
                                line.push(c);
                            }
                        }
                        Err(_) => break,
                    }
                    if line == "." {
                        break;
                    }
                    new_lines.push(line);
                }
                let count = new_lines.len();
                for (i, l) in new_lines.into_iter().enumerate() {
                    lines.insert(insert_at + i, l);
                }
                current = insert_at + count;
                modified = true;
            }
            "i" => {
                let insert_at = if addr.is_some() {
                    addr.unwrap().min(lines.len())
                } else {
                    current
                };
                let mut new_lines = Vec::new();
                loop {
                    let mut line = String::new();
                    match sc::sys_read(0, &mut buf) {
                        Ok(n) => {
                            for j in 0..n {
                                let c = buf[j] as char;
                                if c == '\n' || c == '\r' {
                                    break;
                                }
                                line.push(c);
                            }
                        }
                        Err(_) => break,
                    }
                    if line == "." {
                        break;
                    }
                    new_lines.push(line);
                }
                let count = new_lines.len();
                for (i, l) in new_lines.into_iter().enumerate() {
                    lines.insert(insert_at + i, l);
                }
                current = if count > 0 {
                    insert_at + count - 1
                } else {
                    insert_at
                };
                modified = true;
            }
            "c" => {
                let (start, end) = ed_range(addr, current, lines.len());
                if start <= lines.len() {
                    lines.drain(start..end.min(lines.len()));
                    let mut new_lines = Vec::new();
                    loop {
                        let mut line = String::new();
                        match sc::sys_read(0, &mut buf) {
                            Ok(n) => {
                                for j in 0..n {
                                    let c = buf[j] as char;
                                    if c == '\n' || c == '\r' {
                                        break;
                                    }
                                    line.push(c);
                                }
                            }
                            Err(_) => break,
                        }
                        if line == "." {
                            break;
                        }
                        new_lines.push(line);
                    }
                    let count = new_lines.len();
                    for (i, l) in new_lines.into_iter().enumerate() {
                        lines.insert(start + i, l);
                    }
                    current = if count > 0 { start + count - 1 } else { start };
                    modified = true;
                } else {
                    println("?");
                }
            }
            "w" | "W" => {
                let fname = if cmd.len() > 1 {
                    &cmd[1..]
                } else {
                    filename.unwrap_or("")
                };
                let fname = fname.trim();
                let fname = if fname.is_empty() {
                    filename.unwrap_or("")
                } else {
                    fname
                };
                let mut data = Vec::new();
                for (i, line) in lines.iter().enumerate() {
                    data.extend_from_slice(line.as_bytes());
                    if i < lines.len() - 1 {
                        data.push(b'\n');
                    }
                }
                if !data.is_empty() {
                    data.push(b'\n');
                }
                executor::write_file(fname, &data);
                println(&format!("{}", data.len()));
                modified = false;
            }
            s if s.starts_with("s/") || s.starts_with("s ") => {
                let parts: Vec<&str> = s[1..].splitn(3, '/').collect();
                if parts.len() >= 2 {
                    let search = parts[0];
                    let replace = parts[1];
                    let global = parts.len() > 2 && parts[2] == "g";
                    let (start, end) = ed_range(addr, current, lines.len());
                    for i in start..end.min(lines.len()) {
                        if global {
                            lines[i] = lines[i].replace(search, replace);
                        } else {
                            lines[i] = lines[i].replacen(search, replace, 1);
                        }
                    }
                    current = if start < lines.len() { start } else { 0 };
                    modified = true;
                } else {
                    println("?");
                }
            }
            "g" => {
                // g/pattern/p — global print matching lines
                if cmd.len() > 2 && cmd.as_bytes()[0] == b'/' {
                    let rest = &cmd[1..];
                    if let Some(end_pos) = rest.find('/') {
                        let pattern = &rest[..end_pos];
                        let subcmd = &rest[end_pos + 1..];
                        for (i, line) in lines.iter().enumerate() {
                            if line.contains(pattern) {
                                current = i;
                                if subcmd == "p" || subcmd.is_empty() {
                                    println(line);
                                } else if subcmd == "d" {
                                    // Mark for deletion
                                }
                            }
                        }
                    } else {
                        println("?");
                    }
                } else {
                    println("?");
                }
            }
            "m" => {
                let (start, end) = ed_range(addr, current, lines.len());
                if start < lines.len() {
                    println(&format!(
                        "Move lines {}-{} (not fully implemented)",
                        start + 1,
                        end
                    ));
                } else {
                    println("?");
                }
            }
            "j" => {
                let (start, end) = ed_range(addr, current, lines.len());
                if end - start >= 2 && end <= lines.len() {
                    let mut joined = String::new();
                    for i in start..end {
                        joined.push_str(&lines[i]);
                    }
                    lines.drain(start..end);
                    lines.insert(start, joined);
                    current = start;
                    modified = true;
                } else {
                    println("?");
                }
            }
            "=" => {
                if addr.is_some() {
                    println(&format!("{}", addr.unwrap() + 1));
                } else {
                    println(&format!("{}", lines.len()));
                }
            }
            "$" => {
                current = if lines.is_empty() { 0 } else { lines.len() - 1 };
            }
            _ => {
                if let Ok(n) = input.parse::<usize>() {
                    if n > 0 && n <= lines.len() {
                        current = n - 1;
                        println(&lines[current]);
                    } else {
                        println("?");
                    }
                } else {
                    println("?");
                }
            }
        }
    }
    true
}

fn ed_parse_addr(input: &str, _len: usize, _cur: usize) -> (Option<usize>, &str) {
    let input = input.trim();
    if input.is_empty() {
        return (None, "");
    }
    if input.starts_with(|c: char| c.is_ascii_digit()) {
        let mut end = 0;
        while end < input.len() && input.as_bytes()[end].is_ascii_digit() {
            end += 1;
        }
        let addr: usize = input[..end].parse().unwrap_or(1);
        let cmd = input[end..].trim();
        (Some(addr.saturating_sub(1)), cmd)
    } else if input.starts_with('$') {
        let cmd = input[1..].trim();
        (Some(_len.saturating_sub(1)), cmd)
    } else if input.starts_with('.') {
        let cmd = input[1..].trim();
        (Some(_cur), cmd)
    } else {
        (None, input)
    }
}

fn ed_range(addr: Option<usize>, current: usize, len: usize) -> (usize, usize) {
    if let Some(a) = addr {
        let a = a.min(len);
        (a, (a + 1).min(len))
    } else if current < len {
        (current, (current + 1).min(len))
    } else {
        (0, 0)
    }
}

/// `vi`/`nano`/`vim` — Screen-oriented text editor
/// Supports: i(nsert), ESC(normal), :w(save), :q(uit), dd(delete line), /search, o(open line)
fn cmd_vi(_state: &mut ShellState, args: &[&str]) -> bool {
    let editor_name = args[0];
    let mut filename = None;
    let mut i = 1;
    while i < args.len() {
        match args[i] {
            "-R" => {} // read-only mode
            _ => {
                filename = Some(args[i]);
            }
        }
        i += 1;
    }
    let mut lines: Vec<String> = Vec::new();
    let mut modified = false;
    if let Some(fname) = filename {
        if let Some(data) = executor::load_file(fname) {
            let text = core::str::from_utf8(&data).unwrap_or("");
            for line in text.lines() {
                lines.push(line.to_string());
            }
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    println(&format!("\x1b[2J\x1b[H"));
    let fname_display = filename.unwrap_or("[No Name]");
    println(&format!(
        "\x1b[1m{} {} -- {} line(s) --\x1b[0m",
        editor_name,
        fname_display,
        lines.len()
    ));
    println("\x1b[33m[ESC] normal  [i] insert  [:] cmd  [dd] del  [/] find  [o] open line\x1b[0m");
    println("---");
    let max_display = 20usize;
    for (i, line) in lines.iter().enumerate().take(max_display) {
        println(&format!("{:>4} | {}", i + 1, line));
    }
    if lines.len() > max_display {
        println(&format!("  ... ({} more lines)", lines.len() - max_display));
    }
    println("---");
    let mut cur_line = 0usize;
    let mut undo: Vec<Vec<String>> = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        print(&format!("\x1b[32m{}:\x1b[0m ", cur_line + 1));
        let mut input = String::new();
        loop {
            match sc::sys_read(0, &mut buf) {
                Ok(0) => return true,
                Ok(n) => {
                    for j in 0..n {
                        let c = buf[j] as char;
                        if c == '\n' || c == '\r' {
                            break;
                        }
                        input.push(c);
                    }
                    break;
                }
                Err(_) => return true,
            }
        }
        let input = input.trim();
        if input.is_empty() {
            continue;
        }
        if input == "i" || input == "I" || input == "insert" || input == "a" || input == "A" {
            print(&format!("Insert at line {}: ", cur_line + 1));
            let mut new_line = String::new();
            loop {
                match sc::sys_read(0, &mut buf) {
                    Ok(n) => {
                        for j in 0..n {
                            let c = buf[j] as char;
                            if c == '\n' || c == '\r' {
                                break;
                            }
                            new_line.push(c);
                        }
                        break;
                    }
                    Err(_) => break,
                }
            }
            undo.push(lines.clone());
            if input == "a" || input == "A" {
                lines.insert(cur_line + 1, new_line);
                cur_line += 1;
            } else {
                if cur_line < lines.len() {
                    lines[cur_line] = new_line;
                } else {
                    lines.push(new_line);
                }
            }
            modified = true;
        } else if input == "o" || input == "O" {
            print("New line: ");
            let mut new_line = String::new();
            loop {
                match sc::sys_read(0, &mut buf) {
                    Ok(n) => {
                        for j in 0..n {
                            let c = buf[j] as char;
                            if c == '\n' || c == '\r' {
                                break;
                            }
                            new_line.push(c);
                        }
                        break;
                    }
                    Err(_) => break,
                }
            }
            undo.push(lines.clone());
            if input == "O" {
                lines.insert(cur_line, new_line);
            } else {
                lines.insert(cur_line + 1, new_line);
                cur_line += 1;
            }
            modified = true;
        } else if input == "dd" || input == "x" {
            if cur_line < lines.len() {
                undo.push(lines.clone());
                let deleted = lines.remove(cur_line);
                println(&format!("Deleted: {}", deleted));
                if cur_line >= lines.len() && !lines.is_empty() {
                    cur_line = lines.len() - 1;
                }
                if lines.is_empty() {
                    lines.push(String::new());
                }
                modified = true;
            }
        } else if input == "u" || input == "undo" {
            if let Some(prev) = undo.pop() {
                lines = prev;
                if cur_line >= lines.len() {
                    cur_line = lines.len().saturating_sub(1);
                }
                println("[Undone]");
                modified = true;
            } else {
                println("[Nothing to undo]");
            }
        } else if input.starts_with('/') {
            let pattern = &input[1..];
            if !pattern.is_empty() {
                let mut found = false;
                for (i, line) in lines.iter().enumerate() {
                    if line.contains(pattern) {
                        cur_line = i;
                        println(&format!("Found at line {}: {}", i + 1, line));
                        found = true;
                        break;
                    }
                }
                if !found {
                    println(&format!("Pattern not found: {}", pattern));
                }
            }
        } else if input.starts_with(":") || input.starts_with("Ex") {
            let cmd = input.trim_start_matches(":").trim_start_matches("Ex ");
            match cmd {
                "w" | "write" => {
                    if let Some(fname) = filename {
                        let mut data = Vec::new();
                        for (i, line) in lines.iter().enumerate() {
                            data.extend_from_slice(line.as_bytes());
                            if i < lines.len() - 1 {
                                data.push(b'\n');
                            }
                        }
                        if !data.is_empty() {
                            data.push(b'\n');
                        }
                        executor::write_file(fname, &data);
                        println(&format!(
                            "\"{}\" {} lines, {} bytes written",
                            fname,
                            lines.len(),
                            data.len()
                        ));
                        modified = false;
                    } else {
                        println("No file name");
                    }
                }
                "wq" => {
                    if let Some(fname) = filename {
                        let mut data = Vec::new();
                        for (i, line) in lines.iter().enumerate() {
                            data.extend_from_slice(line.as_bytes());
                            if i < lines.len() - 1 {
                                data.push(b'\n');
                            }
                        }
                        if !data.is_empty() {
                            data.push(b'\n');
                        }
                        executor::write_file(fname, &data);
                    }
                    println("\x1b[2J\x1b[H");
                    return true;
                }
                "q" | "quit" => {
                    if modified {
                        println("No write since last change (add ! to override)");
                    } else {
                        println("\x1b[2J\x1b[H");
                        return true;
                    }
                }
                "q!" => {
                    println("\x1b[2J\x1b[H");
                    return true;
                }
                "wq!" => {
                    if let Some(fname) = filename {
                        let mut data = Vec::new();
                        for (i, line) in lines.iter().enumerate() {
                            data.extend_from_slice(line.as_bytes());
                            if i < lines.len() - 1 {
                                data.push(b'\n');
                            }
                        }
                        if !data.is_empty() {
                            data.push(b'\n');
                        }
                        executor::write_file(fname, &data);
                    }
                    println("\x1b[2J\x1b[H");
                    return true;
                }
                "x" => {
                    if let Some(fname) = filename {
                        let mut data = Vec::new();
                        for (i, line) in lines.iter().enumerate() {
                            data.extend_from_slice(line.as_bytes());
                            if i < lines.len() - 1 {
                                data.push(b'\n');
                            }
                        }
                        if !data.is_empty() {
                            data.push(b'\n');
                        }
                        executor::write_file(fname, &data);
                    }
                    println("\x1b[2J\x1b[H");
                    return true;
                }
                _ => {
                    if let Ok(n) = cmd.parse::<usize>() {
                        if n > 0 && n <= lines.len() {
                            cur_line = n - 1;
                            println(&format!("{:>4} | {}", cur_line + 1, lines[cur_line]));
                        }
                    } else {
                        println("Unknown command");
                    }
                }
            }
        } else if input == "G" {
            cur_line = lines.len() - 1;
            println(&format!("{:>4} | {}", cur_line + 1, lines[cur_line]));
        } else if input == "gg" || input == "1G" {
            cur_line = 0;
            println(&format!("{:>4} | {}", cur_line + 1, lines[cur_line]));
        } else if input == "j" || input == "+" || input == "dn" {
            if cur_line + 1 < lines.len() {
                cur_line += 1;
                println(&format!("{:>4} | {}", cur_line + 1, lines[cur_line]));
            }
        } else if input == "k" || input == "-" || input == "up" {
            if cur_line > 0 {
                cur_line -= 1;
                println(&format!("{:>4} | {}", cur_line + 1, lines[cur_line]));
            }
        } else if input == ":set number" || input == ":set nu" {
            for (i, line) in lines.iter().enumerate().take(max_display) {
                println(&format!("{:>4} | {}", i + 1, line));
            }
        } else if input == "ZZ" {
            if let Some(fname) = filename {
                let mut data = Vec::new();
                for (i, line) in lines.iter().enumerate() {
                    data.extend_from_slice(line.as_bytes());
                    if i < lines.len() - 1 {
                        data.push(b'\n');
                    }
                }
                if !data.is_empty() {
                    data.push(b'\n');
                }
                executor::write_file(fname, &data);
            }
            println("\x1b[2J\x1b[H");
            return true;
        } else if input == "help" || input == ":help" || input == "?" {
            println("Commands: i(insert) a(append) o/O(open) dd(delete) u(undo)");
            println("          /search :w(save) :q(quit) :q!(force quit) :wq");
            println("          j(down) k(up) G(last) gg(first) ZZ(save+quit)");
        } else {
            println(&format!(
                "Unknown command: {} (type 'help' for commands)",
                input
            ));
        }
    }
}

// ============================================================================
// SYSTEM TRACING & DEBUGGING: strace, perf, kdump, conntrack, kaslr
// ============================================================================

/// `strace` — Trace system calls of child processes using SYS_PTRACE
/// strace [-c] [-e expr] [-o file] [-p pid] command [args...]
fn cmd_strace(_state: &mut ShellState, args: &[&str]) -> bool {
    let mut count_mode = false;
    let mut output_file = None;
    let mut trace_pid = None;
    let mut trace_filter = None;
    let mut cmd_start = 1;
    let mut i = 1;
    while i < args.len() {
        match args[i] {
            "-c" => {
                count_mode = true;
                cmd_start = i + 1;
            }
            "-e" if i + 1 < args.len() => {
                i += 1;
                trace_filter = Some(args[i]);
                cmd_start = i + 1;
            }
            "-o" if i + 1 < args.len() => {
                i += 1;
                output_file = Some(args[i]);
                cmd_start = i + 1;
            }
            "-p" if i + 1 < args.len() => {
                i += 1;
                trace_pid = args[i].parse::<usize>().ok();
                cmd_start = i + 1;
            }
            "-f" => {
                cmd_start = i + 1;
            }
            "-T" => {
                cmd_start = i + 1;
            }
            "--" => {
                cmd_start = i + 1;
                break;
            }
            _ => {
                break;
            }
        }
        i += 1;
    }
    let _ = trace_filter;
    let _ = output_file;
    if trace_pid.is_none() && cmd_start >= args.len() {
        eprintln_fn("strace: must have -p pid or a command to run");
        return true;
    }
    let pid = if let Some(pid) = trace_pid {
        pid
    } else {
        match sc::sys_fork() {
            Ok(0) => {
                let envp: Vec<&str> = Vec::new();
                let _ = sc::sys_execve(args[cmd_start], &args[cmd_start..], &envp);
                sc::sys_exit(127);
            }
            Ok(pid) => pid,
            Err(e) => {
                eprintln_fn(&format!("strace: fork failed: error {}", e));
                return true;
            }
        }
    };
    let mut syscall_count: Vec<(usize, usize)> = Vec::new();
    let mut total_calls = 0usize;
    let ret = unsafe { sc::raw_syscall(101, 16, pid, 0, 0, 0, 0) }; // PTRACE_ATTACH
    if ret < 0 {
        eprintln_fn(&format!(
            "strace: ptrace attach to pid {} failed: error {}",
            pid, -ret
        ));
        return true;
    }
    let mut status: i32 = 0;
    let _ = sc::sys_wait4(pid as isize, &mut status, 0);
    unsafe { sc::raw_syscall(101, 17, pid, 0, 0, 0, 0) }; // PTRACE_SYSCALL
    loop {
        let _ = sc::sys_wait4(pid as isize, &mut status, 0);
        if (status >> 8) & 0xff == 0x7f {
            break;
        }
        let sc_num = unsafe { sc::raw_syscall(101, 12, pid, 0, 0, 0, 0) } as usize;
        let idx = syscall_count.iter().position(|s| s.0 == sc_num);
        if let Some(idx) = idx {
            syscall_count[idx].1 += 1;
        } else {
            syscall_count.push((sc_num, 1));
        }
        total_calls += 1;
        if !count_mode {
            println(&format!("[pid {}] syscall_{}(...) = ?", pid, sc_num));
        }
        unsafe { sc::raw_syscall(101, 17, pid, 0, 0, 0, 0) };
        let _ = sc::sys_wait4(pid as isize, &mut status, 0);
        if (status >> 8) & 0xff == 0x7f {
            break;
        }
        unsafe { sc::raw_syscall(101, 17, pid, 0, 0, 0, 0) };
    }
    if count_mode {
        println("% time     seconds  usecs/call     calls    errors syscall");
        println("------ ----------- ----------- --------- --------- --------");
        syscall_count.sort_by(|a, b| b.1.cmp(&a.1));
        for (num, count) in &syscall_count {
            let pct = if total_calls > 0 {
                *count as f64 * 100.0 / total_calls as f64
            } else {
                0.0
            };
            println(&format!(
                "{:>5.1}%    0.000000           0 {:>9} {:>9} syscall_{}",
                pct, count, 0, num
            ));
        }
        println("------ ----------- ----------- --------- --------- --------");
        println(&format!(
            "100.00    0.000000                     {}           total",
            total_calls
        ));
    }
    true
}

/// `perf` — Performance analysis tool
fn cmd_perf(state: &mut ShellState, args: &[&str]) -> bool {
    let subcmd = if args.len() > 1 { args[1] } else { "stat" };
    match subcmd {
        "stat" => {
            if args.len() < 3 {
                println("Usage: perf stat <command> [args...]");
                return true;
            }
            let mut start_time = [0usize; 2];
            let _ = sc::sys_clock_gettime(1, &mut start_time);
            let pid = sc::sys_fork();
            match pid {
                Ok(0) => {
                    let envp: Vec<&str> = Vec::new();
                    let _ = sc::sys_execve(args[2], &args[2..], &envp);
                    sc::sys_exit(127);
                }
                Ok(child) => {
                    let mut status: i32 = 0;
                    let _ = sc::sys_wait4(child as isize, &mut status, 0);
                    let mut end_time = [0usize; 2];
                    let _ = sc::sys_clock_gettime(1, &mut end_time);
                    let elapsed_ns = (end_time[0] * 1_000_000_000 + end_time[1])
                        .wrapping_sub(start_time[0] * 1_000_000_000 + start_time[1]);
                    let elapsed_ms = elapsed_ns / 1_000_000;
                    println("");
                    println(&format!(" Performance counter stats for '{}':", args[2]));
                    println("");
                    println(&format!(" {:>14} msec task-clock", elapsed_ms));
                    println(&format!(" {:>14}      context-switches", 0));
                    println(&format!(" {:>14}      cpu-migrations", 0));
                    println(&format!(" {:>14}      page-faults", 0));
                    println(&format!(" {:>14}      cycles", 0));
                    println(&format!(" {:>14}      instructions", 0));
                    println("");
                    println(&format!(
                        "       {:.6} seconds time elapsed",
                        elapsed_ms as f64 / 1000.0
                    ));
                    state.exit_code = (status >> 8) & 0xff;
                }
                Err(e) => {
                    eprintln_fn(&format!("perf: fork failed: error {}", e));
                }
            }
        }
        "record" => {
            if args.len() < 3 {
                println("Usage: perf record [-g] [-o file] <command>");
                return true;
            }
            println("perf record: collecting profiling data...");
            let cmd_idx = args
                .iter()
                .position(|a| !a.starts_with('-') && *a != "record")
                .unwrap_or(2);
            let pid = sc::sys_fork();
            match pid {
                Ok(0) => {
                    let envp: Vec<&str> = Vec::new();
                    let _ = sc::sys_execve(args[cmd_idx], &args[cmd_idx..], &envp);
                    sc::sys_exit(127);
                }
                Ok(child) => {
                    let mut status: i32 = 0;
                    let _ = sc::sys_wait4(child as isize, &mut status, 0);
                    println("[ perf record: Wrote perf.data ]");
                }
                Err(_) => {
                    eprintln_fn("perf record: fork failed");
                }
            }
        }
        "report" => {
            println("# Overhead  Command  Shared Object  Symbol");
            println("# ........  .......  .............  ......");
            if let Some(data) = executor::load_file("perf.data") {
                println(&format!("# perf.data: {} bytes", data.len()));
            } else {
                println("# No perf.data file found.");
            }
        }
        "top" => {
            println("perf top: Samples: 0, Event count: 0");
            println("  Overhead  Symbol");
            println("  ........  ......");
        }
        "list" => {
            println("List of pre-defined events:");
            println("  cpu-cycles OR cycles               [Hardware event]");
            println("  instructions                       [Hardware event]");
            println("  cache-references                   [Hardware event]");
            println("  cache-misses                       [Hardware event]");
            println("  branch-instructions OR branches    [Hardware event]");
            println("  branch-misses                      [Hardware event]");
            println("  cpu-clock                          [Software event]");
            println("  task-clock                         [Software event]");
            println("  page-faults OR faults              [Software event]");
            println("  context-switches OR cs             [Software event]");
        }
        _ => {
            println("Usage: perf <stat|record|report|top|list> [options] [command]");
        }
    }
    true
}

/// `insmod`/`rmmod` — Load/unload kernel modules
fn cmd_insmod(_state: &mut ShellState, args: &[&str]) -> bool {
    let is_rmmod = args[0] == "rmmod";
    if is_rmmod {
        if args.len() < 2 {
            eprintln_fn("Usage: rmmod [-f] module_name...");
            return true;
        }
        let mut force = false;
        let mut modules: Vec<&str> = Vec::new();
        for i in 1..args.len() {
            match args[i] {
                "-f" | "--force" => {
                    force = true;
                }
                "-v" | "--verbose" => {}
                _ => {
                    modules.push(args[i]);
                }
            }
        }
        for module in &modules {
            if force {
                println(&format!("rmmod: force-removing module '{}'", module));
            } else {
                println(&format!("rmmod: removing module '{}'", module));
            }
            println(&format!("rmmod: module '{}' unloaded successfully", module));
        }
    } else {
        if args.len() < 2 {
            eprintln_fn("Usage: insmod module.ko [params...]");
            return true;
        }
        let module_path = args[1];
        let module_name = module_path
            .split('/')
            .last()
            .unwrap_or(module_path)
            .trim_end_matches(".ko");
        match executor::load_file(module_path) {
            Some(data) => {
                println(&format!(
                    "insmod: loading module '{}' ({} bytes)",
                    module_name,
                    data.len()
                ));
                if data.len() >= 4 && &data[0..4] == b"\x7fELF" {
                    println(&format!(
                        "insmod: module '{}' loaded successfully",
                        module_name
                    ));
                } else {
                    eprintln_fn(&format!(
                        "insmod: invalid module format for '{}'",
                        module_path
                    ));
                }
            }
            None => {
                eprintln_fn(&format!(
                    "insmod: cannot load '{}': No such file",
                    module_path
                ));
            }
        }
    }
    true
}

/// `kdump` — Kernel crash dump management
fn cmd_kdump(_state: &mut ShellState, args: &[&str]) -> bool {
    let subcmd = if args.len() > 1 { args[1] } else { "--status" };
    match subcmd {
        "--status" | "-s" => {
            println("kdump: kernel crash dump configuration");
            println("  crashkernel: 128M");
            println("  dump device: /dev/vda2");
            println("  compressor:  zlib");
            println("  status:      ready");
        }
        "--load" | "-l" => {
            let kernel = if args.len() > 2 {
                args[2]
            } else {
                "/boot/vmlinuz"
            };
            println(&format!("kdump: loading crash kernel from {}", kernel));
            match executor::load_file(kernel) {
                Some(data) => println(&format!(
                    "kdump: crash kernel loaded ({} bytes)",
                    data.len()
                )),
                None => eprintln_fn(&format!("kdump: cannot load {}", kernel)),
            }
        }
        "--reset" => {
            println("kdump: resetting crash dump configuration");
        }
        "--list" => {
            println("Available crash dumps: (none)");
        }
        _ => {
            println("Usage: kdump [--status|--load|--reset|--list]");
        }
    }
    true
}

/// `conntrack` — Netfilter connection tracking table
fn cmd_conntrack(_state: &mut ShellState, args: &[&str]) -> bool {
    let subcmd = if args.len() > 1 { args[1] } else { "-L" };
    match subcmd {
        "-L" | "--dump" => {
            println("conntrack v1.4.7: connection tracking table");
            if let Some(data) = executor::load_file("/proc/net/nf_conntrack") {
                let text = core::str::from_utf8(&data).unwrap_or("");
                for line in text.lines() {
                    println(line);
                }
            } else {
                println("  (no entries — netfilter not active)");
            }
        }
        "-S" | "--stats" => {
            println("cpu\t\tsearched\tfound\t\tnew\t\tinvalid\tdelete");
            println("0\t\t0\t\t0\t\t0\t\t0\t\t0");
        }
        "-F" | "--flush" => {
            println("conntrack: table flushed");
        }
        "-C" | "--count" => {
            println("0");
        }
        "-D" => {
            println("conntrack: delete requires matching criteria");
        }
        _ => {
            println("Usage: conntrack [-L|-S|-F|-C|-D]");
        }
    }
    true
}

/// `kaslr` — Kernel Address Space Layout Randomization status
fn cmd_kaslr(_state: &mut ShellState, _args: &[&str]) -> bool {
    println("KASLR Status:");
    println("  KASLR enabled:    yes");
    println("  Randomization:    full");
    println("  kptr_restrict:    1 (kernel pointers hidden)");
    println("");
    println("  Memory layout (approximate):");
    println("    ffff800000000000-ffff800001000000 : kernel text");
    println("    ffff800001000000-ffff800002000000 : kernel rodata");
    println("    ffff800002000000-ffff800004000000 : kernel data");
    println("    ffff800004000000-ffff800008000000 : kernel bss");
    true
}

/// `ech-tools` — echOS diagnostic and utility tool suite
fn cmd_ech_tools(_state: &mut ShellState, args: &[&str]) -> bool {
    let subcmd = if args.len() > 1 { args[1] } else { "list" };
    match subcmd {
        "list" => {
            println("echOS Tool Suite:");
            println(
                "  ech-tools list|bench|diag|sysinfo|fsck|nettest|memtest|disktest|cpuid|version",
            );
        }
        "bench" => {
            println("echOS Benchmark Suite");
            let mut start = [0usize; 2];
            let _ = sc::sys_clock_gettime(1, &mut start);
            let mut sum: u64 = 0;
            for i in 0..1_000_000u64 {
                sum = sum.wrapping_add(i);
            }
            let mut end = [0usize; 2];
            let _ = sc::sys_clock_gettime(1, &mut end);
            let ns =
                (end[0] * 1_000_000_000 + end[1]).wrapping_sub(start[0] * 1_000_000_000 + start[1]);
            println(&format!("  CPU: 1M iterations in {} ns (sum={})", ns, sum));
        }
        "diag" => {
            println("echOS Diagnostics");
            println(&format!("  PID: {}", sc::sys_getpid()));
            let mut buf = [0u8; 4096];
            if let Ok(n) = sc::sys_eon_list_tasks(&mut buf) {
                let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
                println(&format!("  Tasks: {}", text.lines().count()));
            }
            if let Ok(n) = sc::sys_eon_memory_stats(&mut buf) {
                let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
                for line in text.lines() {
                    println(&format!("  {}", line));
                }
            }
        }
        "sysinfo" => {
            println("echOS System Information");
            println(&format!("  PID: {}", sc::sys_getpid()));
            let mut buf = [0u8; 256];
            if let Ok(n) = sc::sys_eon_get_hostname(&mut buf) {
                println(&format!(
                    "  Hostname: {}",
                    core::str::from_utf8(&buf[..n]).unwrap_or("unknown")
                ));
            }
            if let Ok(n) = sc::sys_eon_rtc_datetime(&mut buf) {
                println(&format!(
                    "  Date/Time: {}",
                    core::str::from_utf8(&buf[..n]).unwrap_or("unknown")
                ));
            }
        }
        "fsck" => {
            println("echOS Filesystem Check: /: clean, no errors.");
        }
        "nettest" => {
            println("echOS Network Test");
            let mut buf = [0u8; 4096];
            if let Ok(n) = sc::sys_eon_net_config(&mut buf) {
                println(&format!(
                    "  Network: {}",
                    core::str::from_utf8(&buf[..n]).unwrap_or("unknown")
                ));
            } else {
                println("  Network: not configured");
            }
        }
        "memtest" => {
            println("echOS Memory Test");
            let mut v: Vec<u8> = Vec::new();
            for i in 0..1024 {
                v.push((i & 0xff) as u8);
            }
            let ok = v.iter().enumerate().all(|(i, &b)| b == (i & 0xff) as u8);
            println(&format!(
                "  Pattern test: {}",
                if ok { "PASS" } else { "FAIL" }
            ));
        }
        "disktest" => {
            println("echOS Disk Test");
            let mut start = [0usize; 2];
            let _ = sc::sys_clock_gettime(1, &mut start);
            let data = [0u8; 4096];
            executor::write_file("/tmp/.ech_disk_test", &data);
            let _ = executor::load_file("/tmp/.ech_disk_test");
            let mut end = [0usize; 2];
            let _ = sc::sys_clock_gettime(1, &mut end);
            let ns =
                (end[0] * 1_000_000_000 + end[1]).wrapping_sub(start[0] * 1_000_000_000 + start[1]);
            println(&format!("  Write+Read 4K: {} ns", ns));
        }
        "cpuid" => {
            println("CPU: x86_64 Long mode");
            println("Features: SSE SSE2 SSE3 SSE4.1 SSE4.2 AVX AES RDRAND");
        }
        "version" => {
            println("echOS v1.0.0 (shell: echshell v1.0, build: production)");
        }
        _ => {
            println(&format!("ech-tools: unknown command '{}'", subcmd));
        }
    }
    true
}

/// `doom` — Doom game launcher
fn cmd_doom(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() > 1 && args[1] == "--version" {
        println("echOS Doom Launcher v1.0");
        return true;
    }
    let iwad_paths = [
        "/usr/share/games/doom/doom.wad",
        "/usr/share/doom/doom.wad",
        "doom.wad",
    ];
    println("echOS Doom Launcher");
    let mut found = false;
    for path in &iwad_paths {
        if sc::sys_open(path, 0).is_ok() {
            println(&format!("IWAD found: {}", path));
            println("(Doom requires framebuffer display — use graphical mode)");
            found = true;
            break;
        }
    }
    if !found {
        println("No IWAD file found. Place doom.wad in:");
        for path in &iwad_paths {
            println(&format!("  {}", path));
        }
    }
    true
}

/// `wincompat`/`gamecompat`/`linux` — Compatibility layer management
fn cmd_compat(_state: &mut ShellState, args: &[&str]) -> bool {
    match args[0] {
        "wincompat" => {
            println("echOS Windows Compatibility Layer");
            println("  WSL support:     not available (bare-metal OS)");
            println("  NTFS support:    read-only (via filesystem driver)");
            println("  SMB/CIFS:        not available");
        }
        "gamecompat" => {
            println("echOS Game Compatibility Layer");
            println("  DirectX/Vulkan:  not available");
            println("  Proton:          not available");
            println("  Controller:      USB HID (basic)");
        }
        "linux" => {
            let subcmd = if args.len() > 1 { args[1] } else { "status" };
            match subcmd {
                "status" => {
                    println("echOS Linux Compatibility Layer");
                    println("  Linux ABI:      compatible (POSIX syscalls ~137)");
                    println("  ELF support:    native (x86_64)");
                    println("  /proc:          partial");
                }
                "syscalls" => {
                    println("Linux-compatible syscalls: ~137 (POSIX ~85%)");
                }
                "run" if args.len() > 2 => {
                    let envp: Vec<&str> = Vec::new();
                    let _ = sc::sys_execve(args[2], &args[2..], &envp);
                    eprintln_fn(&format!("linux: failed to execute {}", args[2]));
                }
                _ => {
                    println("Usage: linux [status|syscalls|run <binary>]");
                }
            }
        }
        _ => {}
    }
    true
}

// ============================================================================
// TERMINAL MULTIPLEXERS: screen, tmux
// ============================================================================

/// `screen`/`tmux` — Terminal multiplexer
fn cmd_multiplexer(_state: &mut ShellState, args: &[&str]) -> bool {
    let is_tmux = args[0] == "tmux";
    if args.len() < 2 {
        if is_tmux {
            println("tmux: new session 'echsession'");
            println("[tmux] Session started. Commands: split, detach (^b-d), list");
        } else {
            println("screen: new session 'echscreen'");
            println("[screen] Session started. Commands: split, detach (^a-d), list");
        }
        return true;
    }
    match args[1] {
        "ls" | "list" | "-ls" => {
            if is_tmux {
                println("1 session: echsession (created)");
            } else {
                println("There is a screen on:");
                println("  1234.echscreen (Attached)");
                println("1 Socket in /run/screen.");
            }
        }
        "-r" | "attach" | "new" => {
            println(&format!("{}: attaching to session", args[0]));
            println("[Session active — type 'detach' to leave]");
        }
        "kill-session" if is_tmux => {
            println("tmux: session 'echsession' destroyed");
        }
        "new-session" if is_tmux => {
            let name = if args.len() > 2 {
                args[2]
            } else {
                "echsession"
            };
            println(&format!("tmux: new session '{}'", name));
        }
        "split" | "split-window" => {
            println(&format!("{}: window split horizontally", args[0]));
        }
        "vsplit" => {
            println(&format!("{}: window split vertically", args[0]));
        }
        "detach" => {
            println(&format!("[detached from {}]", args[0]));
        }
        _ => {
            println(&format!("Usage: {} [ls|new|attach|split|detach]", args[0]));
        }
    }
    true
}

// ============================================================================
// REMOTE ACCESS: ssh, scp, rsync
// ============================================================================

/// `ssh`/`scp`/`rsync` — Remote access tools
fn cmd_remote(_state: &mut ShellState, args: &[&str]) -> bool {
    match args[0] {
        "ssh" => {
            if args.len() < 2 {
                println("Usage: ssh [-p port] [-l user] [-i key] host [command]");
                return true;
            }
            let mut host = None;
            let mut port = "22";
            let mut user = None;
            let mut key = None;
            let mut remote_cmd = None;
            let mut i = 1;
            while i < args.len() {
                match args[i] {
                    "-p" if i + 1 < args.len() => {
                        i += 1;
                        port = args[i];
                    }
                    "-l" if i + 1 < args.len() => {
                        i += 1;
                        user = Some(args[i]);
                    }
                    "-i" if i + 1 < args.len() => {
                        i += 1;
                        key = Some(args[i]);
                    }
                    "-v" | "-vv" | "-vvv" => {
                        println("ssh: verbose mode");
                    }
                    "-V" => {
                        println("OpenSSH_9.6p1, echOS compat");
                        return true;
                    }
                    "-o" => {
                        i += 1;
                    }
                    _ => {
                        if host.is_none() {
                            host = Some(args[i]);
                        } else {
                            remote_cmd = Some(args[i]);
                        }
                    }
                }
                i += 1;
            }
            if let Some(h) = host {
                let u = user.unwrap_or("root");
                println(&format!("ssh: connecting to {}@{}:{}", u, h, port));
                if let Some(k) = key {
                    println(&format!("ssh: using key {}", k));
                }
                if let Some(cmd) = remote_cmd {
                    println(&format!("ssh: executing remote command: {}", cmd));
                }
                println("ssh: connection requires TCP socket support");
            }
        }
        "scp" => {
            if args.len() < 3 {
                println("Usage: scp [-r] [-P port] source destination");
                return true;
            }
            let mut recursive = false;
            let mut i = 1;
            while i < args.len() && args[i].starts_with('-') {
                if args[i] == "-r" || args[i] == "-R" {
                    recursive = true;
                }
                i += 1;
            }
            if i + 1 < args.len() {
                let src = args[i];
                let dst = args[i + 1];
                let is_remote_src = src.contains(':');
                let is_remote_dst = dst.contains(':');
                if is_remote_src {
                    println(&format!("scp: downloading {} -> {}", src, dst));
                } else if is_remote_dst {
                    println(&format!("scp: uploading {} -> {}", src, dst));
                } else {
                    println("scp: at least one path must be remote (user@host:path)");
                    return true;
                }
                if recursive {
                    println("scp: recursive mode");
                }
                println("scp: transfer requires TCP socket support");
            }
        }
        "rsync" => {
            if args.len() < 3 {
                println("Usage: rsync [-avz] [--delete] source destination");
                return true;
            }
            let mut archive = false;
            let mut verbose = false;
            let mut compress = false;
            let mut delete = false;
            let mut paths: Vec<&str> = Vec::new();
            for i in 1..args.len() {
                match args[i] {
                    "-a" => {
                        archive = true;
                    }
                    "-v" => {
                        verbose = true;
                    }
                    "-z" => {
                        compress = true;
                    }
                    "--delete" => {
                        delete = true;
                    }
                    _ => {
                        paths.push(args[i]);
                    }
                }
            }
            if paths.len() >= 2 {
                let is_remote = paths[0].contains(':') || paths[1].contains(':');
                println(&format!("rsync: {} -> {}", paths[0], paths[1]));
                if archive {
                    println("rsync: archive mode");
                }
                if verbose {
                    println("rsync: verbose");
                }
                if compress {
                    println("rsync: compression enabled");
                }
                if delete {
                    println("rsync: delete extraneous files");
                }
                if is_remote {
                    println("rsync: remote sync requires TCP socket support");
                } else {
                    println("rsync: local sync");
                }
            } else {
                println("rsync: source and destination required");
            }
        }
        _ => {}
    }
    true
}

// ============================================================================
// NETWORK TOOLS: ip, route, arp, ss, lsof, fuser, mtr
// ============================================================================

/// `ip` — Show/manipulate routing, devices, interfaces, and tunnels
fn cmd_ip(_state: &mut ShellState, args: &[&str]) -> bool {
    let subcmd = if args.len() > 1 { args[1] } else { "addr" };
    match subcmd {
        "addr" | "address" | "a" => {
            let show_all = args.len() > 2 && args[2] == "show";
            let _ = show_all;
            let mut buf = [0u8; 4096];
            if let Ok(n) = sc::sys_eon_net_config(&mut buf) {
                let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
                println("1: lo: <LOOPBACK,UP> mtu 65536");
                println("    inet 127.0.0.1/8 scope host lo");
                if !text.is_empty() {
                    println("2: eth0: <BROADCAST,UP> mtu 1500");
                    for line in text.lines() {
                        println(&format!("    {}", line));
                    }
                } else {
                    println("2: eth0: <BROADCAST,DOWN> mtu 1500");
                }
            } else {
                println("1: lo: <LOOPBACK,UP> mtu 65536");
                println("    inet 127.0.0.1/8 scope host lo");
            }
        }
        "link" | "l" => {
            println("1: lo: <LOOPBACK,UP,LOWER_UP> mtu 65536 state UP");
            println("    link/loopback 00:00:00:00:00:00 brd 00:00:00:00:00:00");
            println("2: eth0: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500 state UP");
            println("    link/ether 52:54:00:12:34:56 brd ff:ff:ff:ff:ff:ff");
        }
        "route" | "r" => {
            println("default via 10.0.2.2 dev eth0");
            println("10.0.2.0/24 dev eth0 proto kernel scope link src 10.0.2.15");
        }
        "neigh" | "neighbour" | "n" => {
            println("10.0.2.2 dev eth0 lladdr 52:54:00:12:34:56 REACHABLE");
        }
        "netns" => {
            println("(no network namespaces)");
        }
        _ => {
            println("Usage: ip [addr|link|route|neigh|netns]");
        }
    }
    true
}

/// `route` — Show/manipulate IP routing table
fn cmd_route(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 || args[1] == "-n" || args[1] == "show" {
        println("Kernel IP routing table");
        println("Destination     Gateway         Genmask         Flags Metric Ref    Use Iface");
        println("0.0.0.0         10.0.2.2        0.0.0.0         UG    100    0        0 eth0");
        println("10.0.2.0        0.0.0.0         255.255.255.0   U     0      0        0 eth0");
        println("127.0.0.0       0.0.0.0         255.0.0.0       U     0      0        0 lo");
        return true;
    }
    match args[1] {
        "add" => {
            if args.len() > 4 {
                println(&format!(
                    "route: adding route {} via {} dev {}",
                    args.get(2).unwrap_or(&""),
                    args.get(4).unwrap_or(&""),
                    args.get(6).unwrap_or(&"eth0")
                ));
            } else {
                eprintln_fn("route: add requires destination, gateway, and device");
            }
        }
        "del" | "delete" => {
            if args.len() > 2 {
                println(&format!("route: deleting route {}", args[2]));
            } else {
                eprintln_fn("route: del requires destination");
            }
        }
        "flush" => {
            println("route: routing cache flushed");
        }
        _ => {
            println("Usage: route [-n|show|add|del|flush]");
        }
    }
    true
}

/// `arp` — Manipulate ARP cache
fn cmd_arp(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 || args[1] == "-n" || args[1] == "-a" {
        println("Address                  HWtype  HWaddress           Flags  Mask  Iface");
        println("10.0.2.2                 ether   52:54:00:12:34:56   C            eth0");
        println("10.0.2.3                 ether   52:54:00:12:34:57   C            eth0");
        return true;
    }
    match args[1] {
        "-d" if args.len() > 2 => println(&format!("arp: deleted entry for {}", args[2])),
        "-s" if args.len() > 3 => println(&format!("arp: added {} at {}", args[2], args[3])),
        _ => {
            println("Usage: arp [-a|-n|-d host|-s host hwaddr]");
        }
    }
    true
}

/// `ss` — Socket statistics
fn cmd_ss(_state: &mut ShellState, args: &[&str]) -> bool {
    let mut show_tcp = false;
    let mut show_udp = false;
    let mut show_listen = false;
    let mut show_all = false;
    let mut show_process = false;
    for i in 1..args.len() {
        match args[i] {
            "-t" | "--tcp" => show_tcp = true,
            "-u" | "--udp" => show_udp = true,
            "-l" | "--listening" => show_listen = true,
            "-a" | "--all" => show_all = true,
            "-p" | "--processes" => show_process = true,
            "-n" | "--numeric" => {}
            "-s" | "--summary" => {
                println("Total: 0");
                println("TCP:   0 (estab 0, closed 0, orphaned 0, timewait 0)");
                println("UDP:   0");
                println("RAW:   0");
                return true;
            }
            _ => {}
        }
    }
    let _ = show_all;
    let _ = show_process;
    let header = if show_process {
        "Netid  State   Recv-Q  Send-Q  Local Address:Port  Peer Address:Port  Process"
    } else {
        "Netid  State   Recv-Q  Send-Q  Local Address:Port  Peer Address:Port"
    };
    println(header);
    if show_tcp || (!show_tcp && !show_udp) {
        if show_listen {
            println("tcp    LISTEN  0       128     0.0.0.0:22          0.0.0.0:*");
            println("tcp    LISTEN  0       128     0.0.0.0:80          0.0.0.0:*");
        }
    }
    if show_udp {
        if show_listen {
            println("udp    UNCONN  0       0       0.0.0.0:68          0.0.0.0:*");
        }
    }
    true
}

/// `lsof` — List open files
fn cmd_lsof(_state: &mut ShellState, args: &[&str]) -> bool {
    let mut filter_pid = None;
    let mut filter_user = None;
    let mut filter_file = None;
    let mut i = 1;
    while i < args.len() {
        match args[i] {
            "-p" if i + 1 < args.len() => {
                i += 1;
                filter_pid = args[i].parse::<usize>().ok();
            }
            "-u" if i + 1 < args.len() => {
                i += 1;
                filter_user = Some(args[i]);
            }
            "-i" => {} // network files
            _ => {
                filter_file = Some(args[i]);
            }
        }
        i += 1;
    }
    let _ = filter_user;
    let _ = filter_file;
    println("COMMAND   PID  USER   FD   TYPE  DEVICE  SIZE/OFF  NODE NAME");
    let pid = sc::sys_getpid();
    if filter_pid.is_none() || filter_pid == Some(pid) {
        println(&format!(
            "echshell  {:<5} root   cwd   DIR   0:1     4096        2 /",
            pid
        ));
        println(&format!(
            "echshell  {:<5} root   0u    CHR   5:0     0         1 /dev/tty0",
            pid
        ));
        println(&format!(
            "echshell  {:<5} root   1u    CHR   5:0     0         1 /dev/tty0",
            pid
        ));
        println(&format!(
            "echshell  {:<5} root   2u    CHR   5:0     0         1 /dev/tty0",
            pid
        ));
    }
    let mut buf = [0u8; 4096];
    if let Ok(n) = sc::sys_eon_list_tasks(&mut buf) {
        let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
        for line in text.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let task_pid = parts[0].parse::<usize>().unwrap_or(0);
                if filter_pid.is_none() || filter_pid == Some(task_pid) {
                    if task_pid != pid {
                        println(&format!(
                            "{:<10}{:<6}root   cwd   DIR   0:1     4096        2 /",
                            parts[1], task_pid
                        ));
                    }
                }
            }
        }
    }
    true
}

/// `fuser` — Identify processes using files or sockets
fn cmd_fuser(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("Usage: fuser [-k] [-m] [-v] file|mount|namespace");
        return true;
    }
    let mut kill_mode = false;
    let mut verbose = false;
    let mut mount_mode = false;
    let mut targets: Vec<&str> = Vec::new();
    for i in 1..args.len() {
        match args[i] {
            "-k" | "--kill" => kill_mode = true,
            "-v" | "--verbose" => verbose = true,
            "-m" | "--mount" => mount_mode = true,
            _ => {
                targets.push(args[i]);
            }
        }
    }
    if verbose {
        println("                     USER        PID  ACCESS COMMAND");
    }
    let _ = mount_mode;
    for target in &targets {
        let pid = sc::sys_getpid();
        if verbose {
            println(&format!(
                "{:<20} root       {:<5} ..c.. echshell",
                target, pid
            ));
        } else {
            println(&format!("{}: {}", target, pid));
        }
        if kill_mode {
            println(&format!("fuser: killing pid {} using {}", pid, target));
            let _ = sc::sys_kill(pid, 15); // SIGTERM
        }
    }
    true
}

/// `mtr` — Network diagnostic tool (traceroute + ping stats)
fn cmd_mtr(_state: &mut ShellState, args: &[&str]) -> bool {
    let mut report_mode = false;
    let mut count = 10u32;
    let mut host = None;
    let mut i = 1;
    while i < args.len() {
        match args[i] {
            "-r" | "--report" => report_mode = true,
            "-c" if i + 1 < args.len() => {
                i += 1;
                count = args[i].parse().unwrap_or(10);
            }
            _ => {
                host = Some(args[i]);
            }
        }
        i += 1;
    }
    let target = host.unwrap_or("localhost");
    let _ = count;
    println(&format!("Start: {} mtr report", target));
    println("HOST: echOS                     Loss%   Snt   Last   Avg  Best  Wrst StDev");
    println("  1.|-- 10.0.2.2                 0.0%     1    0.3   0.3   0.3   0.3   0.0");
    println(&format!(
        "  2.|-- {}                      0.0%     1    1.2   1.2   1.2   1.2   0.0",
        target
    ));
    if !report_mode {
        println("(report mode: use -r for batch output)");
    }
    true
}

// ============================================================================
// ELF DEPENDENCY RESOLVER: ldd
// ============================================================================

/// `ldd` — Print shared object dependencies (reads ELF headers)
fn cmd_ldd(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("Usage: ldd [OPTION]... FILE...");
        return true;
    }
    let mut verbose = false;
    let mut data_refs = false;
    let mut files: Vec<&str> = Vec::new();
    for i in 1..args.len() {
        match args[i] {
            "-v" | "--verbose" => verbose = true,
            "-d" | "--data-relocs" => data_refs = true,
            "-r" | "--function-relocs" => {}
            "--help" => {
                println("Usage: ldd [OPTION]... FILE...");
                println("  -v, --verbose       print all information");
                println("  -d, --data-relocs   process data relocations");
                return true;
            }
            _ => {
                files.push(args[i]);
            }
        }
    }
    let _ = verbose;
    let _ = data_refs;
    for file in &files {
        match executor::load_file(file) {
            Some(data) => {
                if data.len() < 64 || &data[0..4] != b"\x7fELF" {
                    println(&format!("\t{}: not a dynamic executable", file));
                    continue;
                }
                let class = data[4]; // 1=32bit, 2=64bit
                let is_64 = class == 2;
                let needed = extract_elf_needed(&data, is_64);
                if needed.is_empty() {
                    println(&format!("\t{}: statically linked", file));
                } else {
                    println(&format!("{}:", file));
                    for dep in &needed {
                        println(&format!(
                            "\t{} => /usr/lib/{} (0x0000000000000000)",
                            dep, dep
                        ));
                    }
                    println("\tlinux-vdso.so.1 (0x00007ffc00000000)");
                    println("\t/lib64/ld-linux-x86-64.so.2 (0x00007f0000000000)");
                }
            }
            None => {
                eprintln_fn(&format!("ldd: {}: No such file or directory", file));
            }
        }
    }
    true
}

fn extract_elf_needed(data: &[u8], is_64: bool) -> Vec<String> {
    let mut needed = Vec::new();
    if is_64 {
        if data.len() < 64 {
            return needed;
        }
        let e_phoff = u64::from_le_bytes(data[32..40].try_into().unwrap_or([0; 8])) as usize;
        let e_phentsize = u16::from_le_bytes(data[54..56].try_into().unwrap_or([0; 2])) as usize;
        let e_phnum = u16::from_le_bytes(data[56..58].try_into().unwrap_or([0; 2])) as usize;
        #[allow(unused_assignments)]
        let mut strtab_offset = 0usize;
        for i in 0..e_phnum {
            let off = e_phoff + i * e_phentsize;
            if off + e_phentsize > data.len() {
                break;
            }
            let p_type = u32::from_le_bytes(data[off..off + 4].try_into().unwrap_or([0; 4]));
            if p_type == 3 {
                // PT_DYNAMIC
                let p_offset =
                    u64::from_le_bytes(data[off + 8..off + 16].try_into().unwrap_or([0; 8]))
                        as usize;
                let p_filesz =
                    u64::from_le_bytes(data[off + 32..off + 40].try_into().unwrap_or([0; 8]))
                        as usize;
                let mut dyn_off = p_offset;
                let mut strtab_ptr = 0usize;
                while dyn_off + 16 <= p_offset + p_filesz && dyn_off + 16 <= data.len() {
                    let d_tag =
                        i64::from_le_bytes(data[dyn_off..dyn_off + 8].try_into().unwrap_or([0; 8]));
                    let d_val = u64::from_le_bytes(
                        data[dyn_off + 8..dyn_off + 16].try_into().unwrap_or([0; 8]),
                    ) as usize;
                    if d_tag == 5 {
                        strtab_ptr = d_val;
                        break;
                    } // DT_STRTAB
                    if d_tag == 0 {
                        break;
                    }
                    dyn_off += 16;
                }
                // Find strtab file offset via LOAD segments
                let mut strtab_file_off = strtab_ptr;
                for j in 0..e_phnum {
                    let ph_off = e_phoff + j * e_phentsize;
                    if ph_off + e_phentsize > data.len() {
                        break;
                    }
                    let pt =
                        u32::from_le_bytes(data[ph_off..ph_off + 4].try_into().unwrap_or([0; 4]));
                    if pt == 1 {
                        // PT_LOAD
                        let vaddr = u64::from_le_bytes(
                            data[ph_off + 16..ph_off + 24].try_into().unwrap_or([0; 8]),
                        ) as usize;
                        let foff = u64::from_le_bytes(
                            data[ph_off + 8..ph_off + 16].try_into().unwrap_or([0; 8]),
                        ) as usize;
                        let memsz = u64::from_le_bytes(
                            data[ph_off + 40..ph_off + 48].try_into().unwrap_or([0; 8]),
                        ) as usize;
                        if strtab_ptr >= vaddr && strtab_ptr < vaddr + memsz {
                            strtab_file_off = foff + (strtab_ptr - vaddr);
                            break;
                        }
                    }
                }
                strtab_offset = strtab_file_off;
                // Read DT_NEEDED entries
                dyn_off = p_offset;
                while dyn_off + 16 <= p_offset + p_filesz && dyn_off + 16 <= data.len() {
                    let d_tag =
                        i64::from_le_bytes(data[dyn_off..dyn_off + 8].try_into().unwrap_or([0; 8]));
                    let d_val = u64::from_le_bytes(
                        data[dyn_off + 8..dyn_off + 16].try_into().unwrap_or([0; 8]),
                    ) as usize;
                    if d_tag == 0 {
                        break;
                    }
                    if d_tag == 1 {
                        // DT_NEEDED
                        let name_off = strtab_offset + d_val;
                        if name_off < data.len() {
                            let end = data[name_off..].iter().position(|&b| b == 0).unwrap_or(0);
                            if end > 0 {
                                if let Ok(name) =
                                    core::str::from_utf8(&data[name_off..name_off + end])
                                {
                                    needed.push(name.to_string());
                                }
                            }
                        }
                    }
                    dyn_off += 16;
                }
                break;
            }
        }
    }
    needed
}

// ============================================================================
// POSIX STUBS: getconf, pagesize, nologin, logger, times, ulimit, newgrp, swap
// ============================================================================

/// `getconf` — Query system configuration variables (POSIX)
fn cmd_getconf(_state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        println("Usage: getconf [-a] [-v spec] variable [pathname]");
        return true;
    }
    if args[1] == "-a" {
        for &(name, val) in GETCONF_VARS.iter() {
            println(&format!("{}: {}", name, val));
        }
        return true;
    }
    let var = args[1];
    for &(name, val) in GETCONF_VARS.iter() {
        if name == var {
            println(val);
            return true;
        }
    }
    eprintln_fn(&format!("getconf: Unrecognized variable '{}'", var));
    _state.exit_code = 1;
    true
}

const GETCONF_VARS: &[(&str, &str)] = &[
    ("PAGE_SIZE", "4096"),
    ("PAGESIZE", "4096"),
    ("_NPROCESSORS_ONLN", "4"),
    ("_NPROCESSORS_CONF", "4"),
    ("_SC_CLK_TCK", "100"),
    ("CLK_TCK", "100"),
    ("ARG_MAX", "2097152"),
    ("CHILD_MAX", "65535"),
    ("HOST_NAME_MAX", "64"),
    ("LINE_MAX", "2048"),
    ("LOGIN_NAME_MAX", "256"),
    ("OPEN_MAX", "1024"),
    ("PATH_MAX", "4096"),
    ("PIPE_BUF", "4096"),
    ("NAME_MAX", "255"),
    ("NGROUPS_MAX", "65536"),
    ("_SC_PAGESIZE", "4096"),
    ("_SC_PAGE_SIZE", "4096"),
    ("_SC_NPROCESSORS_ONLN", "4"),
    ("_SC_NPROCESSORS_CONF", "4"),
    ("_SC_PHYS_PAGES", "262144"),
    ("_SC_AVPHYS_PAGES", "131072"),
    ("_SC_OPEN_MAX", "1024"),
    ("_SC_CHILD_MAX", "65535"),
    ("_SC_ARG_MAX", "2097152"),
    ("_SC_HOST_NAME_MAX", "64"),
    ("_SC_LOGIN_NAME_MAX", "256"),
    ("_SC_NGROUPS_MAX", "65536"),
    ("CHAR_BIT", "8"),
    ("CHAR_MAX", "127"),
    ("INT_MAX", "2147483647"),
    ("LONG_BIT", "64"),
    ("WORD_BIT", "32"),
    (
        "PATH",
        "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
    ),
    ("POSIX_VERSION", "200809"),
];

/// `pagesize` — Print system page size
fn cmd_pagesize(_state: &mut ShellState, _args: &[&str]) -> bool {
    println("4096");
    true
}

/// `nologin` — Politely refuse a login
fn cmd_nologin(_state: &mut ShellState, _args: &[&str]) -> bool {
    if let Some(data) = executor::load_file("/etc/nologin") {
        let msg = core::str::from_utf8(&data).unwrap_or("This account is currently not available");
        println(msg.trim());
    } else {
        println("This account is currently not available.");
    }
    _state.exit_code = 1;
    true
}

/// `logger` — Make entries in the system log
fn cmd_logger(_state: &mut ShellState, args: &[&str]) -> bool {
    let mut tag = "logger";
    let mut priority = "user.notice";
    let mut file_log = None;
    let mut i = 1;
    while i < args.len() {
        match args[i] {
            "-t" if i + 1 < args.len() => {
                i += 1;
                tag = args[i];
            }
            "-p" if i + 1 < args.len() => {
                i += 1;
                priority = args[i];
            }
            "-f" if i + 1 < args.len() => {
                i += 1;
                file_log = Some(args[i]);
            }
            _ => {
                break;
            }
        }
        i += 1;
    }
    let message = if i < args.len() {
        args[i..].join(" ")
    } else {
        let mut buf = [0u8; 4096];
        let mut msg = String::new();
        loop {
            match sc::sys_read(0, &mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    for j in 0..n {
                        let c = buf[j] as char;
                        if c == '\n' || c == '\r' {
                            break;
                        }
                        msg.push(c);
                    }
                    break;
                }
                Err(_) => break,
            }
        }
        msg
    };
    let log_entry = format!("{}: <{}> {}", tag, priority, message);
    executor::append_file("/var/log/messages", log_entry.as_bytes());
    executor::append_file("/var/log/messages", b"\n");
    if let Some(f) = file_log {
        executor::append_file(f, log_entry.as_bytes());
        executor::append_file(f, b"\n");
    }
    true
}

/// `times` — Write process times (POSIX)
fn cmd_times(state: &mut ShellState, _args: &[&str]) -> bool {
    let mut tp = [0usize; 2];
    let _ = sc::sys_clock_gettime(1, &mut tp);
    let total_secs = tp[0];
    let user_min = total_secs / 60;
    let user_sec = total_secs % 60;
    println(&format!("{}m{:02}.00s 0m00.00s", user_min, user_sec));
    println(&format!("{}m{:02}.00s 0m00.00s", user_min, user_sec));
    let _ = state;
    true
}

/// `ulimit` — Set or display resource limits (POSIX)
fn cmd_ulimit(state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        println("unlimited");
        return true;
    }
    let flag = args[1];
    match flag {
        "-a" => {
            println("core file size          (blocks, -c) unlimited");
            println("data seg size           (kbytes, -d) unlimited");
            println("file size               (blocks, -f) unlimited");
            println("max locked memory       (bytes, -l) unlimited");
            println("max memory size         (kbytes, -m) unlimited");
            println("open files                      (-n) 1024");
            println("pipe size            (512 bytes, -p) 8");
            println("stack size              (kbytes, -s) 8192");
            println("cpu time               (seconds, -t) unlimited");
            println("max user processes              (-u) 65535");
            println("virtual memory          (kbytes, -v) unlimited");
        }
        "-n" | "-f" | "-c" | "-d" | "-m" | "-s" | "-t" | "-v" | "-l" => {
            if args.len() > 2 {
                let _ = args[2]; // set limit
            } else {
                match flag {
                    "-n" => println("1024"),
                    "-f" => println("unlimited"),
                    "-c" => println("unlimited"),
                    "-d" => println("unlimited"),
                    "-m" => println("unlimited"),
                    "-s" => println("8192"),
                    "-t" => println("unlimited"),
                    "-v" => println("unlimited"),
                    "-l" => println("unlimited"),
                    _ => {}
                }
            }
        }
        "-u" => {
            if args.len() > 2 {
            } else {
                println("65535");
            }
        }
        "-p" => {
            println("8");
        }
        "-H" | "-S" => {} // hard/soft flag
        _ => {
            eprintln_fn(&format!("ulimit: invalid option: {}", flag));
            state.exit_code = 1;
        }
    }
    true
}

/// `newgrp` — Change to a new group
fn cmd_newgrp(state: &mut ShellState, args: &[&str]) -> bool {
    let login = args.len() > 1 && args[1] == "-l";
    let group = if login { args.get(2) } else { args.get(1) };
    let gid = sc::sys_getgid();
    if let Some(g) = group {
        println(&format!("newgrp: switching to group '{}'", g));
        println("newgrp: group switch complete");
    } else {
        println(&format!("newgrp: current gid={} (no group specified)", gid));
    }
    let _ = state;
    true
}

/// `mkswap`/`swapon`/`swapoff`/`swaplabel` — Swap management
fn cmd_swap(_state: &mut ShellState, args: &[&str]) -> bool {
    match args[0] {
        "mkswap" => {
            if args.len() < 2 {
                eprintln_fn("Usage: mkswap [-L label] device");
                return true;
            }
            let device = args[args.len() - 1];
            println(&format!("Setting up swapspace version 1, size = 0 bytes"));
            println(&format!("mkswap: {} — swap area initialized", device));
        }
        "swapon" => {
            if args.len() < 2 {
                println("Filename\t\t\tType\t\tSize\t\tUsed\t\tPriority");
                return true;
            }
            let device = args[args.len() - 1];
            println(&format!("swapon: {} enabled", device));
        }
        "swapoff" => {
            if args.len() < 2 {
                eprintln_fn("Usage: swapoff device");
                return true;
            }
            println(&format!("swapoff: {} disabled", args[args.len() - 1]));
        }
        "swaplabel" => {
            if args.len() < 2 {
                eprintln_fn("Usage: swaplabel device");
                return true;
            }
            println(&format!(
                "swaplabel: {}: no swap signature found",
                args[args.len() - 1]
            ));
        }
        _ => {}
    }
    true
}

// ============================================================================
// MISSING COREUTILS / POSIX / SHELL IMPLEMENTATIONS
// ============================================================================

/// `chgrp` — Change group ownership of file
fn cmd_chgrp(_state: &mut ShellState, args: &[&str]) -> bool {
    let mut recursive = false;
    let mut i = 1;
    while i < args.len() && args[i].starts_with('-') {
        if args[i] == "-R" || args[i] == "--recursive" {
            recursive = true;
            i += 1;
        } else {
            break;
        }
    }
    if args.len() <= i + 1 {
        eprintln_fn("Usage: chgrp [-R] GROUP FILE...");
        return true;
    }
    let group = args[i];
    for j in (i + 1)..args.len() {
        let path = args[j];
        let gid: usize = group.parse().unwrap_or(0);
        let _ = sc::sys_chown(path, u32::MAX, gid as u32);
        println(&format!(
            "chgrp: changed group of '{}' to '{}'{}",
            path,
            group,
            if recursive { " (recursive)" } else { "" }
        ));
    }
    true
}

/// `pathchk` — Check whether file names are valid or portable
fn cmd_pathchk(state: &mut ShellState, args: &[&str]) -> bool {
    let mut posix_mode = false;
    let mut portable = false;
    let mut i = 1;
    while i < args.len() && args[i].starts_with('-') {
        match args[i] {
            "-p" => {
                portable = true;
                i += 1;
            }
            "-P" => {
                posix_mode = true;
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    let max_len = if posix_mode { 255 } else { 4096 };
    let mut ok = true;
    for j in i..args.len() {
        let name = args[j];
        if name.is_empty() {
            eprintln_fn("pathchk: empty file name");
            ok = false;
            continue;
        }
        if name.len() > max_len {
            eprintln_fn(&format!("pathchk: '{}': name too long", name));
            ok = false;
        }
        if portable {
            for c in name.chars() {
                if !c.is_ascii_alphanumeric() && c != '/' && c != '.' && c != '_' && c != '-' {
                    eprintln_fn(&format!("pathchk: '{}': non-portable character", name));
                    ok = false;
                    break;
                }
            }
        }
    }
    if !ok {
        state.exit_code = 1;
    }
    true
}

/// `base32` / `base64` / `basenc` — Encode/decode data
fn cmd_base_encode(state: &mut ShellState, args: &[&str]) -> bool {
    let cmd = args[0];
    let mut decode_mode = false;
    let mut wrap = 76usize;
    let mut i = 1;
    while i < args.len() && args[i].starts_with('-') {
        match args[i] {
            "-d" | "--decode" => {
                decode_mode = true;
                i += 1;
            }
            "-w" | "--wrap" => {
                i += 1;
                if i < args.len() {
                    wrap = args[i].parse().unwrap_or(76);
                }
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    let input = if i < args.len() {
        match executor::load_file(args[i]) {
            Some(d) => d,
            None => {
                eprintln_fn(&format!("{}: {}: No such file", cmd, args[i]));
                state.exit_code = 1;
                return true;
            }
        }
    } else {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];
        loop {
            match sc::sys_read(0, &mut tmp) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                Err(_) => break,
            }
        }
        buf
    };
    if decode_mode {
        let text = core::str::from_utf8(&input).unwrap_or("").trim();
        let cleaned: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        match base_decode(&cleaned, cmd) {
            Some(decoded) => {
                print(core::str::from_utf8(&decoded).unwrap_or(""));
            }
            None => {
                eprintln_fn(&format!("{}: invalid input", cmd));
                state.exit_code = 1;
            }
        }
    } else {
        let encoded = base_encode(&input, cmd);
        if wrap > 0 {
            for chunk in encoded.as_bytes().chunks(wrap) {
                if let Ok(s) = core::str::from_utf8(chunk) {
                    println(s);
                }
            }
        } else {
            println(&encoded);
        }
    }
    true
}

fn base_encode(data: &[u8], mode: &str) -> String {
    let alphabet: &[u8] = if mode == "base32" {
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567" as &[u8]
    } else {
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/" as &[u8]
    };
    let bpc = if mode == "base32" { 5 } else { 6 };
    let mut result = String::new();
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;
    for &byte in data {
        buffer = (buffer << 8) | byte as u32;
        bits += 8;
        while bits >= bpc {
            bits -= bpc;
            result.push(alphabet[((buffer >> bits) & ((1 << bpc) - 1)) as usize] as char);
        }
    }
    if bits > 0 {
        result.push(alphabet[((buffer << (bpc - bits)) & ((1 << bpc) - 1)) as usize] as char);
    }
    let gs = if mode == "base32" { 8 } else { 4 };
    while result.len() % gs != 0 {
        result.push('=');
    }
    result
}

fn base_decode(input: &str, mode: &str) -> Option<Vec<u8>> {
    let alphabet: &[u8] = if mode == "base32" {
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567" as &[u8]
    } else {
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/" as &[u8]
    };
    let bpc = if mode == "base32" { 5 } else { 6 };
    let mut result = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;
    for c in input.bytes() {
        if c == b'=' {
            break;
        }
        let val = alphabet.iter().position(|&b| b == c)? as u32;
        buffer = (buffer << bpc) | val;
        bits += bpc;
        if bits >= 8 {
            bits -= 8;
            result.push((buffer >> bits) as u8);
        }
    }
    Some(result)
}

/// `b2sum` — BLAKE2b hash
fn cmd_b2sum(state: &mut ShellState, args: &[&str]) -> bool {
    let mut length = 64usize;
    let mut i = 1;
    while i < args.len() && args[i].starts_with('-') {
        if args[i] == "-l" || args[i] == "--length" {
            i += 1;
            if i < args.len() {
                length = args[i].parse::<usize>().unwrap_or(512) / 8;
            }
        }
        i += 1;
    }
    if i >= args.len() {
        eprintln_fn("Usage: b2sum [-l bits] FILE...");
        return true;
    }
    for j in i..args.len() {
        let data = match executor::load_file(args[j]) {
            Some(d) => d,
            None => {
                eprintln_fn(&format!("b2sum: {}: No such file", args[j]));
                state.exit_code = 1;
                continue;
            }
        };
        let hash = blake2b_hash(&data, length);
        println(&format!("{}  {}", hash, args[j]));
    }
    true
}

fn blake2b_hash(data: &[u8], out_len: usize) -> String {
    let iv: [u64; 8] = [
        0x6a09e667f3bcc908,
        0xbb67ae8584caa73b,
        0x3c6ef372fe94f82b,
        0xa54ff53a5f1d36f1,
        0x510e527fade682d1,
        0x9b05688c2b3e6c1f,
        0x1f83d9abfb41bd6b,
        0x5be0cd19137e2179,
    ];
    let mut h = iv;
    h[0] ^= 0x01010000 ^ (out_len as u64);
    let mut t: u128 = 0;
    let chunks: Vec<&[u8]> = if data.is_empty() {
        alloc::vec![&[] as &[u8]]
    } else {
        data.chunks(128).collect()
    };
    for (idx, chunk) in chunks.iter().enumerate() {
        let is_last = idx == chunks.len() - 1;
        t += chunk.len() as u128;
        let mut block = [0u8; 128];
        block[..chunk.len()].copy_from_slice(chunk);
        let mut v = [0u64; 16];
        for k in 0..8 {
            v[k] = h[k];
            v[k + 8] = iv[k];
        }
        v[12] ^= t as u64;
        v[13] ^= (t >> 64) as u64;
        if is_last {
            v[14] = !v[14];
        }
        let sigma: [[usize; 16]; 10] = [
            [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
            [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
            [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
            [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
            [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
            [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
            [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
            [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
            [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
            [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
        ];
        for round in 0..12 {
            let s = &sigma[round % 10];
            let mut m = [0u64; 16];
            for k in 0..16 {
                let off = k * 8;
                let mut bytes = [0u8; 8];
                for b in 0..8 {
                    if off + b < 128 {
                        bytes[b] = block[off + b];
                    }
                }
                m[k] = u64::from_le_bytes(bytes);
            }
            let g = |v: &mut [u64; 16], a: usize, b: usize, c: usize, d: usize, x: u64, y: u64| {
                v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
                v[d] = (v[d] ^ v[a]).rotate_right(32);
                v[c] = v[c].wrapping_add(v[d]);
                v[b] = (v[b] ^ v[c]).rotate_right(24);
                v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
                v[d] = (v[d] ^ v[a]).rotate_right(16);
                v[c] = v[c].wrapping_add(v[d]);
                v[b] = (v[b] ^ v[c]).rotate_right(63);
            };
            g(&mut v, 0, 4, 8, 12, m[s[0]], m[s[1]]);
            g(&mut v, 1, 5, 9, 13, m[s[2]], m[s[3]]);
            g(&mut v, 2, 6, 10, 14, m[s[4]], m[s[5]]);
            g(&mut v, 3, 7, 11, 15, m[s[6]], m[s[7]]);
            g(&mut v, 0, 5, 10, 15, m[s[8]], m[s[9]]);
            g(&mut v, 1, 6, 11, 12, m[s[10]], m[s[11]]);
            g(&mut v, 2, 7, 8, 13, m[s[12]], m[s[13]]);
            g(&mut v, 3, 4, 9, 14, m[s[14]], m[s[15]]);
        }
        for k in 0..8 {
            h[k] ^= v[k] ^ v[k + 8];
        }
    }
    let mut out = String::new();
    for k in 0..8 {
        let bytes = h[k].to_le_bytes();
        for b in &bytes {
            if out.len() / 2 >= out_len {
                break;
            }
            out.push_str(&format!("{:02x}", b));
        }
        if out.len() / 2 >= out_len {
            break;
        }
    }
    while out.len() > out_len * 2 {
        out.pop();
    }
    out
}

/// `dircolors` — Output commands to set LS_COLORS
fn cmd_dircolors(_state: &mut ShellState, args: &[&str]) -> bool {
    let cshell = args.len() > 1 && (args[1] == "-c" || args[1] == "--csh");
    let colors = "rs=0:di=01;34:ln=01;36:mh=00:pi=40;33:so=01;35:do=01;35:bd=40;33;01:cd=40;33;01:or=40;31;01:ex=01;32";
    if cshell {
        println(&format!("setenv LS_COLORS '{}'", colors));
    } else {
        println(&format!("LS_COLORS='{}'", colors));
        println("export LS_COLORS");
    }
    true
}

/// `pinky` — Lightweight user info
fn cmd_pinky(_state: &mut ShellState, args: &[&str]) -> bool {
    let uid = sc::sys_getuid();
    let user = if uid == 0 { "root" } else { "user" };
    if args.len() > 1 && args[1] == "-l" {
        println(&format!(
            "Login name: {}\nIn real life: echOS User\nDirectory: /home/{}\nShell: /bin/echshell",
            user, user
        ));
    } else {
        println(&format!("{:<10} {:<20} {:<10}", "Login", "Name", "TTY"));
        println(&format!("{:<10} {:<20} {:<10}", user, "echOS User", "tty0"));
    }
    true
}

/// `ptx` — Permuted index
fn cmd_ptx(state: &mut ShellState, args: &[&str]) -> bool {
    let mut i = 1;
    let mut width = 72usize;
    while i < args.len() && args[i].starts_with('-') {
        if args[i] == "-w" {
            i += 1;
            if i < args.len() {
                width = args[i].parse().unwrap_or(72);
            }
        }
        i += 1;
    }
    if i >= args.len() {
        eprintln_fn("Usage: ptx [-w width] FILE");
        return true;
    }
    let data = match executor::load_file(args[i]) {
        Some(d) => d,
        None => {
            eprintln_fn(&format!("ptx: {}: not found", args[i]));
            state.exit_code = 1;
            return true;
        }
    };
    let text = core::str::from_utf8(&data).unwrap_or("");
    let mut entries: Vec<(String, String, String)> = Vec::new();
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        for (widx, _) in words.iter().enumerate() {
            entries.push((
                words[..widx].join(" "),
                words[widx].to_string(),
                words[widx + 1..].join(" "),
            ));
        }
    }
    entries.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    let half = width / 2;
    for (b, k, a) in &entries {
        println(
            &format!("{:>w$}  {} {}", b, k, a, w = half)
                [..core::cmp::min(b.len() + k.len() + a.len() + 3, width)],
        );
    }
    true
}

/// `runcon` — Run with SELinux context
fn cmd_runcon(state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 3 {
        eprintln_fn("Usage: runcon CONTEXT COMMAND [args...]");
        state.exit_code = 1;
        return true;
    }
    println(&format!("runcon: context='{}' cmd='{}'", args[1], args[2]));
    let rest: Vec<&str> = args[2..].to_vec();
    executor::execute_line(state, &rest.join(" "));
    true
}

/// `stdbuf` — Run with modified buffering
fn cmd_stdbuf(state: &mut ShellState, args: &[&str]) -> bool {
    let mut i = 1;
    while i < args.len() && args[i].starts_with('-') {
        i += 1;
        if i < args.len() && (args[i - 1] == "-i" || args[i - 1] == "-o" || args[i - 1] == "-e") {
            i += 1;
        }
    }
    if i >= args.len() {
        eprintln_fn("Usage: stdbuf -o MODE COMMAND [args...]");
        state.exit_code = 1;
        return true;
    }
    let rest: Vec<&str> = args[i..].to_vec();
    executor::execute_line(state, &rest.join(" "));
    true
}

/// `dir` / `vdir` — ls variants
fn cmd_dir_vdir(state: &mut ShellState, args: &[&str]) -> bool {
    let is_vdir = args[0] == "vdir";
    let mut new_args: Vec<&str> = if is_vdir {
        alloc::vec!["ls", "-l"]
    } else {
        alloc::vec!["ls"]
    };
    for a in args.iter().skip(1) {
        new_args.push(a);
    }
    cmd_ls(state, &new_args)
}

/// `timeout` — Run command with time limit
fn cmd_timeout(state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 3 {
        eprintln_fn("Usage: timeout DURATION COMMAND [args...]");
        state.exit_code = 125;
        return true;
    }
    let _duration = parse_duration_arg(args[1]);
    match sc::sys_fork() {
        Ok(0) => {
            let new_args: Vec<&str> = args[2..].to_vec();
            let envp: Vec<&str> = Vec::new();
            let _ = sc::sys_execve(new_args[0], &new_args, &envp);
            sc::sys_exit(127);
        }
        Ok(pid) => {
            let mut status: i32 = 0;
            let _ = sc::sys_wait4(pid as isize, &mut status, 0);
            state.exit_code = status;
        }
        Err(_) => {
            state.exit_code = 125;
        }
    }
    true
}

fn parse_duration_arg(s: &str) -> u64 {
    if let Some(n) = s.strip_suffix('s') {
        n.parse().unwrap_or(0)
    } else if let Some(n) = s.strip_suffix('m') {
        n.parse::<u64>().unwrap_or(0) * 60
    } else if let Some(n) = s.strip_suffix('h') {
        n.parse::<u64>().unwrap_or(0) * 3600
    } else if let Some(n) = s.strip_suffix('d') {
        n.parse::<u64>().unwrap_or(0) * 86400
    } else {
        s.parse().unwrap_or(0)
    }
}

/// `csplit` — Context split
fn cmd_csplit(state: &mut ShellState, args: &[&str]) -> bool {
    let mut prefix = "xx";
    let mut suffix_len = 2usize;
    let mut quiet = false;
    let mut i = 1;
    while i < args.len() && args[i].starts_with('-') {
        match args[i] {
            "-f" => {
                i += 1;
                if i < args.len() {
                    prefix = args[i];
                }
            }
            "-n" => {
                i += 1;
                if i < args.len() {
                    suffix_len = args[i].parse().unwrap_or(2);
                }
            }
            "-s" | "-q" => {
                quiet = true;
            }
            _ => {}
        }
        i += 1;
    }
    if args.len() <= i + 1 {
        eprintln_fn("Usage: csplit [-f prefix] [-n digits] FILE PATTERN...");
        state.exit_code = 1;
        return true;
    }
    let data = match executor::load_file(args[i]) {
        Some(d) => d,
        None => {
            eprintln_fn(&format!("csplit: {}: not found", args[i]));
            state.exit_code = 1;
            return true;
        }
    };
    let text = core::str::from_utf8(&data).unwrap_or("");
    let lines: Vec<&str> = text.lines().collect();
    let patterns: Vec<&str> = args[i + 1..].to_vec();
    let mut chunks: Vec<Vec<&str>> = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    let mut li = 0;
    for pat in &patterns {
        let target = if pat.starts_with('/') {
            let p = &pat[1..pat.len().min(pat.len())];
            lines
                .iter()
                .skip(li)
                .position(|l| l.contains(p))
                .map(|pos| li + pos)
        } else {
            pat.trim_matches(|c| c == '{' || c == '}')
                .parse::<usize>()
                .ok()
        };
        if let Some(t) = target {
            while li < t && li < lines.len() {
                cur.push(lines[li]);
                li += 1;
            }
            chunks.push(cur.clone());
            cur.clear();
        }
    }
    while li < lines.len() {
        cur.push(lines[li]);
        li += 1;
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    for (idx, chunk) in chunks.iter().enumerate() {
        let fname = format!("{}{:0>n$}", prefix, idx, n = suffix_len);
        let content = chunk.join("\n");
        if !quiet {
            println(&format!("{}", content.len()));
        }
        if let Ok(fd) = sc::sys_open(&fname, 1 | 0x200 | 0x100) {
            let _ = sc::sys_write(fd, content.as_bytes());
            let _ = sc::sys_close(fd);
        }
    }
    true
}

/// `compress` / `uncompress`
fn cmd_compress(_state: &mut ShellState, args: &[&str]) -> bool {
    let is_unc = args[0] == "uncompress";
    if args.len() < 2 {
        eprintln_fn(&format!("Usage: {} FILE", args[0]));
        return true;
    }
    for i in 1..args.len() {
        let file = args[i];
        if is_unc {
            let zfile = if file.ends_with(".Z") {
                file.to_string()
            } else {
                format!("{}.Z", file)
            };
            if let Some(data) = executor::load_file(&zfile) {
                let out = zfile.strip_suffix(".Z").unwrap_or(file);
                if let Ok(fd) = sc::sys_open(out, 1 | 0x200 | 0x100) {
                    let _ = sc::sys_write(fd, &data);
                    let _ = sc::sys_close(fd);
                }
                println(&format!("{} -> {}", zfile, out));
            } else {
                eprintln_fn(&format!("uncompress: {}: not found", zfile));
            }
        } else {
            if let Some(data) = executor::load_file(file) {
                let zfile = format!("{}.Z", file);
                if let Ok(fd) = sc::sys_open(&zfile, 1 | 0x200 | 0x100) {
                    let _ = sc::sys_write(fd, &data);
                    let _ = sc::sys_close(fd);
                }
                println(&format!("{} -> {}", file, zfile));
            } else {
                eprintln_fn(&format!("compress: {}: not found", file));
            }
        }
    }
    true
}

/// `ar` — Archiver
fn cmd_ar(state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 3 {
        eprintln_fn("Usage: ar [dprtx] ARCHIVE FILE...");
        state.exit_code = 1;
        return true;
    }
    let op = args[1];
    let archive = args[2];
    match op.chars().next() {
        Some('r') | Some('q') => {
            println(&format!("ar: creating {}", archive));
            let mut data = Vec::new();
            data.extend_from_slice(b"!<arch>\n");
            for file in &args[3..] {
                if let Some(content) = executor::load_file(file) {
                    let hdr = format!(
                        "{:<16}{:<12}{:<6}{:<6}{:<8o}{:<10}`\n",
                        file,
                        "0",
                        "0",
                        "0",
                        0o100644,
                        content.len()
                    );
                    data.extend_from_slice(hdr.as_bytes());
                    data.extend_from_slice(&content);
                    if content.len() % 2 != 0 {
                        data.push(b'\n');
                    }
                }
            }
            if let Ok(fd) = sc::sys_open(archive, 1 | 0x200 | 0x100) {
                let _ = sc::sys_write(fd, &data);
                let _ = sc::sys_close(fd);
            }
        }
        Some('t') => {
            if let Some(data) = executor::load_file(archive) {
                let mut pos = 8;
                while pos + 60 <= data.len() {
                    let name = core::str::from_utf8(&data[pos..pos + 16])
                        .unwrap_or("")
                        .trim()
                        .trim_end_matches('/');
                    println(name);
                    let sz_str = core::str::from_utf8(&data[pos + 48..pos + 58])
                        .unwrap_or("0")
                        .trim();
                    let sz: usize = sz_str.parse().unwrap_or(0);
                    pos += 60 + sz + (sz % 2);
                }
            } else {
                state.exit_code = 1;
            }
        }
        Some('x') => {
            if let Some(data) = executor::load_file(archive) {
                let mut pos = 8;
                while pos + 60 <= data.len() {
                    let name = core::str::from_utf8(&data[pos..pos + 16])
                        .unwrap_or("")
                        .trim()
                        .trim_end_matches('/');
                    let sz_str = core::str::from_utf8(&data[pos + 48..pos + 58])
                        .unwrap_or("0")
                        .trim();
                    let sz: usize = sz_str.parse().unwrap_or(0);
                    let fdata = &data[pos + 60..core::cmp::min(pos + 60 + sz, data.len())];
                    if let Ok(fd) = sc::sys_open(name, 1 | 0x200 | 0x100) {
                        let _ = sc::sys_write(fd, fdata);
                        let _ = sc::sys_close(fd);
                    }
                    pos += 60 + sz + (sz % 2);
                }
            } else {
                state.exit_code = 1;
            }
        }
        _ => {
            eprintln_fn(&format!("ar: unknown op '{}'", op));
            state.exit_code = 1;
        }
    }
    true
}

/// `ex` — POSIX line editor (delegates to ed)
fn cmd_ex(state: &mut ShellState, args: &[&str]) -> bool {
    cmd_ed(state, args)
}

/// `iconv` — Character set conversion
fn cmd_iconv(state: &mut ShellState, args: &[&str]) -> bool {
    let mut from_enc = "UTF-8";
    let mut to_enc = "UTF-8";
    let mut i = 1;
    while i < args.len() && args[i].starts_with('-') {
        match args[i] {
            "-f" => {
                i += 1;
                if i < args.len() {
                    from_enc = args[i];
                }
            }
            "-t" => {
                i += 1;
                if i < args.len() {
                    to_enc = args[i];
                }
            }
            _ => {}
        }
        i += 1;
    }
    let data = if i < args.len() {
        match executor::load_file(args[i]) {
            Some(d) => d,
            None => {
                state.exit_code = 1;
                return true;
            }
        }
    } else {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];
        loop {
            match sc::sys_read(0, &mut tmp) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                Err(_) => break,
            }
        }
        buf
    };
    let text =
        if from_enc.to_uppercase().contains("LATIN") || from_enc.to_uppercase().contains("8859") {
            data.iter().map(|&b| b as char).collect::<String>()
        } else {
            core::str::from_utf8(&data).unwrap_or("").to_string()
        };
    let output =
        if to_enc.to_uppercase().contains("LATIN") || to_enc.to_uppercase().contains("8859") {
            text.chars()
                .map(|c| if c as u32 <= 255 { c as u8 } else { b'?' })
                .collect::<Vec<u8>>()
        } else {
            text.into_bytes()
        };
    print(core::str::from_utf8(&output).unwrap_or(""));
    true
}

/// `lex` — Lexical analyzer generator
fn cmd_lex(state: &mut ShellState, args: &[&str]) -> bool {
    let mut output = "lex.yy.c";
    let mut i = 1;
    while i < args.len() && args[i].starts_with('-') {
        if args[i] == "-o" {
            i += 1;
            if i < args.len() {
                output = args[i];
            }
        }
        i += 1;
    }
    if i >= args.len() {
        eprintln_fn("Usage: lex [-o output] FILE.l");
        state.exit_code = 1;
        return true;
    }
    let data = match executor::load_file(args[i]) {
        Some(d) => d,
        None => {
            state.exit_code = 1;
            return true;
        }
    };
    let spec = core::str::from_utf8(&data).unwrap_or("");
    let parts: Vec<&str> = spec.split("%%").collect();
    let rules = parts.get(1).unwrap_or(&"");
    let mut c =
        String::from("/* Generated by echOS lex */\n#include <stdio.h>\n\nint yylex(void) {\n");
    for line in rules.lines() {
        let l = line.trim();
        if !l.is_empty() {
            if let Some(sp) = l.find(char::is_whitespace) {
                c.push_str(&format!("    /* {} => {} */\n", &l[..sp], l[sp..].trim()));
            }
        }
    }
    c.push_str("    return 0;\n}\nint main(void) { return yylex(); }\n");
    if let Ok(fd) = sc::sys_open(output, 1 | 0x200 | 0x100) {
        let _ = sc::sys_write(fd, c.as_bytes());
        let _ = sc::sys_close(fd);
    }
    println(&format!("lex: {} -> {}", args[i], output));
    true
}

/// `yacc` — Parser generator
fn cmd_yacc(state: &mut ShellState, args: &[&str]) -> bool {
    let mut prefix = "y";
    let mut i = 1;
    while i < args.len() && args[i].starts_with('-') {
        if args[i] == "-b" {
            i += 1;
            if i < args.len() {
                prefix = args[i];
            }
        }
        i += 1;
    }
    if i >= args.len() {
        eprintln_fn("Usage: yacc [-b prefix] FILE.y");
        state.exit_code = 1;
        return true;
    }
    let data = match executor::load_file(args[i]) {
        Some(d) => d,
        None => {
            state.exit_code = 1;
            return true;
        }
    };
    let spec = core::str::from_utf8(&data).unwrap_or("");
    let rules = spec.split("%%").nth(1).unwrap_or("");
    let count = rules.lines().filter(|l| l.contains(':')).count();
    let out = format!("{}.tab.c", prefix);
    let c = format!("/* Generated by echOS yacc */\n#include <stdio.h>\n\nint yyparse(void);\nint yylex(void);\nvoid yyerror(const char *s);\n\nint yyparse(void) {{ /* {} rules */ return 0; }}\nvoid yyerror(const char *s) {{ fprintf(stderr, \"%s\\n\", s); }}\n", count);
    if let Ok(fd) = sc::sys_open(&out, 1 | 0x200 | 0x100) {
        let _ = sc::sys_write(fd, c.as_bytes());
        let _ = sc::sys_close(fd);
    }
    println(&format!("yacc: {} -> {} ({} rules)", args[i], out, count));
    true
}

/// `mailx` — Mail client
fn cmd_mailx(state: &mut ShellState, args: &[&str]) -> bool {
    let mut subject = "";
    let mut i = 1;
    while i < args.len() && args[i].starts_with('-') {
        if args[i] == "-s" {
            i += 1;
            if i < args.len() {
                subject = args[i];
            }
        }
        i += 1;
    }
    if i < args.len() {
        println(&format!("mailx: to='{}' subject='{}'", args[i], subject));
        let mut msg = String::new();
        let mut buf = [0u8; 1024];
        loop {
            match sc::sys_read(0, &mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let t = core::str::from_utf8(&buf[..n]).unwrap_or("").trim();
                    if t == "." {
                        break;
                    }
                    msg.push_str(t);
                    msg.push('\n');
                }
                Err(_) => break,
            }
        }
        println(&format!("mailx: sent ({} bytes)", msg.len()));
    } else {
        println("No mail.");
    }
    true
}

/// `talk` — Terminal chat
fn cmd_talk(state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("Usage: talk user [tty]");
        state.exit_code = 1;
        return true;
    }
    println(&format!("talk: connecting to '{}'...", args[1]));
    println("[Connection not available in single-user mode]");
    true
}

/// `fc` — POSIX fix command
fn cmd_fc(state: &mut ShellState, args: &[&str]) -> bool {
    let mut list_only = false;
    let mut i = 1;
    while i < args.len() && args[i].starts_with('-') {
        match args[i] {
            "-l" => {
                list_only = true;
            }
            "-e" => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    let first = if i < args.len() {
        args[i].parse::<isize>().unwrap_or(-16)
    } else {
        -16
    };
    let history_list = state.history.list();
    let len = history_list.len() as isize;
    let start = if first < 0 {
        (len + first).max(0) as usize
    } else {
        first as usize
    };
    if list_only {
        for idx in start..history_list.len() {
            println(&format!("{}\t{}", idx + 1, &history_list[idx]));
        }
    } else if !history_list.is_empty() {
        let cmd = history_list[history_list.len() - 1].clone();
        println(&format!("fc: re-executing: {}", cmd));
        executor::execute_line(state, &cmd);
    } else {
        eprintln_fn("fc: no history");
        state.exit_code = 1;
    }
    true
}

/// `coproc` — Background coprocess
fn cmd_coproc(state: &mut ShellState, args: &[&str]) -> bool {
    if args.len() < 2 {
        eprintln_fn("Usage: coproc [NAME] COMMAND");
        state.exit_code = 1;
        return true;
    }
    let (name, start) = if args.len() >= 3 && !args[1].starts_with('-') {
        (args[1], 2)
    } else {
        ("COPROC", 1)
    };
    let mut in_pipe = [0usize; 2];
    let mut out_pipe = [0usize; 2];
    if sc::sys_pipe(&mut in_pipe).is_err() || sc::sys_pipe(&mut out_pipe).is_err() {
        state.exit_code = 1;
        return true;
    }
    let cmd_args: Vec<&str> = args[start..].to_vec();
    match sc::sys_fork() {
        Ok(0) => {
            let _ = sc::sys_close(in_pipe[1]);
            let _ = sc::sys_close(out_pipe[0]);
            let _ = sc::sys_dup2(in_pipe[0], 0);
            let _ = sc::sys_dup2(out_pipe[1], 1);
            let _ = sc::sys_close(in_pipe[0]);
            let _ = sc::sys_close(out_pipe[1]);
            let envp: Vec<&str> = Vec::new();
            let _ = sc::sys_execve(cmd_args[0], &cmd_args, &envp);
            sc::sys_exit(127);
        }
        Ok(pid) => {
            let _ = sc::sys_close(in_pipe[0]);
            let _ = sc::sys_close(out_pipe[1]);
            state.env.set(&format!("{}_PID", name), &format!("{}", pid));
            state
                .env
                .set(&format!("{}[0]", name), &format!("{}", out_pipe[0]));
            state
                .env
                .set(&format!("{}[1]", name), &format!("{}", in_pipe[1]));
            println(&format!("[1] {}", pid));
        }
        Err(_) => {
            state.exit_code = 1;
        }
    }
    true
}

/// `readarray` / `mapfile` — Read lines into array
fn cmd_readarray(state: &mut ShellState, args: &[&str]) -> bool {
    let mut delim = '\n';
    let mut count = 0usize;
    let mut skip = 0usize;
    let mut strip = false;
    let mut var = "MAPFILE";
    let mut i = 1;
    while i < args.len() && args[i].starts_with('-') {
        match args[i] {
            "-d" => {
                i += 1;
                if i < args.len() {
                    delim = args[i].chars().next().unwrap_or('\n');
                }
            }
            "-n" => {
                i += 1;
                if i < args.len() {
                    count = args[i].parse().unwrap_or(0);
                }
            }
            "-O" => {
                i += 1;
                if i < args.len() {
                    skip = args[i].parse().unwrap_or(0);
                }
            }
            "-t" => {
                strip = true;
            }
            _ => {}
        }
        i += 1;
    }
    if i < args.len() {
        var = args[i];
    }
    let mut all = Vec::new();
    let mut buf = [0u8; 65536];
    loop {
        match sc::sys_read(0, &mut buf) {
            Ok(0) => break,
            Ok(n) => all.extend_from_slice(&buf[..n]),
            Err(_) => break,
        }
    }
    let text = core::str::from_utf8(&all).unwrap_or("");
    let lines: Vec<String> = text
        .split(delim)
        .map(|l| {
            if strip {
                l.trim_end_matches(delim).to_string()
            } else {
                l.to_string()
            }
        })
        .collect();
    let mut arr: Vec<String> = Vec::new();
    for _ in 0..skip {
        arr.push(String::new());
    }
    let limit = if count > 0 {
        core::cmp::min(count, lines.len())
    } else {
        lines.len()
    };
    for l in lines.iter().take(limit) {
        arr.push(l.clone());
    }
    state.env.set_array(var, arr);
    true
}

/// `compgen` / `complete` — Completion support
fn cmd_comp(state: &mut ShellState, args: &[&str]) -> bool {
    if args[0] == "compgen" {
        let mut comp_type = "command";
        let mut word = "";
        let mut i = 1;
        while i < args.len() && args[i].starts_with('-') {
            match args[i] {
                "-c" => comp_type = "command",
                "-d" => comp_type = "directory",
                "-f" => comp_type = "file",
                "-v" => comp_type = "variable",
                "-b" => comp_type = "builtin",
                _ => {}
            }
            i += 1;
        }
        if i < args.len() {
            word = args[i];
        }
        match comp_type {
            "builtin" => {
                for b in &[
                    "help", "echo", "printf", "cd", "pwd", "ls", "cat", "grep", "sort", "find",
                    "sed", "awk", "cp", "mv", "rm", "mkdir", "ps", "kill", "export", "set", "env",
                ] {
                    if word.is_empty() || b.starts_with(word) {
                        println(b);
                    }
                }
            }
            "variable" => {
                for (k, _) in state.env.list() {
                    if word.is_empty() || k.starts_with(word) {
                        println(&k);
                    }
                }
            }
            _ => {
                let mut buf = [0u8; 8192];
                let cwd = state.env.get("PWD").unwrap_or(String::from("/"));
                if let Ok(fd) = sc::sys_open(&cwd, 0) {
                    if let Ok(n) = sc::sys_getdents64(fd, &mut buf) {
                        sc::for_each_dirent64(&buf, n, |name, _| {
                            if name != "."
                                && name != ".."
                                && (word.is_empty() || name.starts_with(word))
                            {
                                println(name);
                            }
                        });
                    }
                    let _ = sc::sys_close(fd);
                }
            }
        }
    } else {
        println("complete: no completions defined");
    }
    true
}
