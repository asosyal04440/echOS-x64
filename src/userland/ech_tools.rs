//! `ech-tools` command catalog and dispatcher contract.
//!
//! Upstream command sources stay under `third_party/curated/`. This module owns
//! the echOS-facing command map, bring-up tier, and shell dispatch status.

use alloc::string::String;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandSource {
    Sbase,
    Ubase,
}

impl CommandSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sbase => "sbase",
            Self::Ubase => "ubase",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum CommandTier {
    Tier0,
    Tier1,
    Tier2,
}

impl CommandTier {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tier0 => "tier0",
            Self::Tier1 => "tier1",
            Self::Tier2 => "tier2",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandState {
    ShellBridge,
    AdapterPending,
}

impl CommandState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ShellBridge => "shell-bridge",
            Self::AdapterPending => "adapter-pending",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandDescriptor {
    pub name: &'static str,
    pub summary: &'static str,
    pub usage: &'static str,
    pub source: CommandSource,
    pub tier: CommandTier,
    pub state: CommandState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Dispatch<'a> {
    List,
    Help(Option<&'a CommandDescriptor>),
    RunShellBridge {
        descriptor: &'a CommandDescriptor,
        args: &'a [&'a str],
    },
    AdapterPending(&'a CommandDescriptor),
    Unknown(&'a str),
}

pub const SOURCE_COMMANDS_TOTAL: usize = 152;
pub const SOURCE_COMMANDS_UNIQUE: usize = 150;
pub const SOURCE_DUPLICATES: &[&str] = &["dd", "mknod"];

const TIER0: CommandTier = CommandTier::Tier0;
const TIER1: CommandTier = CommandTier::Tier1;
const TIER2: CommandTier = CommandTier::Tier2;
const SBASE: CommandSource = CommandSource::Sbase;
const UBASE: CommandSource = CommandSource::Ubase;
const BRIDGE: CommandState = CommandState::ShellBridge;

pub const COMMANDS: &[CommandDescriptor] = &[
    CommandDescriptor {
        name: "basename",
        summary: "print the last non-empty path component",
        usage: "basename <path>",
        source: SBASE,
        tier: TIER0,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "bc",
        summary: "evaluate calculator expressions",
        usage: "bc [file]",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "blkdiscard",
        summary: "discard block-device sectors",
        usage: "blkdiscard <device>",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "cal",
        summary: "print a calendar",
        usage: "cal [month] [year]",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "cat",
        summary: "write file or stdin bytes to stdout",
        usage: "cat <file>...",
        source: SBASE,
        tier: TIER0,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "chgrp",
        summary: "change file group ownership",
        usage: "chgrp <group> <path>...",
        source: SBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "chmod",
        summary: "change file permission bits",
        usage: "chmod <mode> <path>...",
        source: SBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "chown",
        summary: "change file user and group ownership",
        usage: "chown <owner>[:group] <path>...",
        source: SBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "chroot",
        summary: "run a command with a different root directory",
        usage: "chroot <root> [command]",
        source: SBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "chvt",
        summary: "switch the active virtual terminal",
        usage: "chvt <tty-number>",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "cksum",
        summary: "compute POSIX CRC checksums and byte counts",
        usage: "cksum <file>...",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "clear",
        summary: "clear the terminal screen",
        usage: "clear",
        source: UBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "cmp",
        summary: "compare two files byte by byte",
        usage: "cmp <left> <right>",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "cols",
        summary: "format input into columns",
        usage: "cols [file]",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "comm",
        summary: "compare two sorted files line by line",
        usage: "comm <left> <right>",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "cp",
        summary: "copy files and directory entries",
        usage: "cp <source> <target>",
        source: SBASE,
        tier: TIER0,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "cron",
        summary: "run scheduled commands from crontab entries",
        usage: "cron",
        source: SBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "ctrlaltdel",
        summary: "configure Ctrl-Alt-Del handling",
        usage: "ctrlaltdel <hard|soft>",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "cut",
        summary: "select byte, character, or field ranges",
        usage: "cut -f <list> [file]",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "date",
        summary: "print or set system date and time",
        usage: "date [format]",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "dc",
        summary: "evaluate reverse-polish calculator expressions",
        usage: "dc [file]",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "dd",
        summary: "copy and convert block-oriented data",
        usage: "dd if=<src> of=<dst> [bs=N]",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "df",
        summary: "report mounted filesystem capacity",
        usage: "df [path]",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "dirname",
        summary: "print the directory portion of a path",
        usage: "dirname <path>",
        source: SBASE,
        tier: TIER0,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "dmesg",
        summary: "print kernel message buffer contents",
        usage: "dmesg",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "du",
        summary: "estimate file and directory disk usage",
        usage: "du [path]...",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "echo",
        summary: "write arguments to stdout",
        usage: "echo [text]...",
        source: SBASE,
        tier: TIER0,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "ed",
        summary: "edit text with a line editor",
        usage: "ed [file]",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "eject",
        summary: "eject removable media",
        usage: "eject <device>",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "env",
        summary: "print or run with environment variables",
        usage: "env [name=value]... [command]",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "expand",
        summary: "convert tabs to spaces",
        usage: "expand [file]",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "expr",
        summary: "evaluate shell expressions",
        usage: "expr <expression>",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "fallocate",
        summary: "reserve storage for a file",
        usage: "fallocate <path> <length>",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "false",
        summary: "return an unsuccessful exit status",
        usage: "false",
        source: SBASE,
        tier: TIER0,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "find",
        summary: "walk paths and select entries by predicates",
        usage: "find [path] [expression]",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "flock",
        summary: "manage advisory file locks",
        usage: "flock <file> <command>",
        source: SBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "fold",
        summary: "wrap input lines to a target width",
        usage: "fold [-w width] [file]",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "free",
        summary: "report memory and swap usage",
        usage: "free",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "freeramdisk",
        summary: "free a RAM disk device",
        usage: "freeramdisk <device>",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "fsfreeze",
        summary: "freeze or thaw filesystem writes",
        usage: "fsfreeze <-f|-u> <mount>",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "getconf",
        summary: "print system configuration variables",
        usage: "getconf <name>",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "getty",
        summary: "start a terminal login session",
        usage: "getty <tty>",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "grep",
        summary: "print lines matching a pattern",
        usage: "grep <pattern> [file]",
        source: SBASE,
        tier: TIER0,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "halt",
        summary: "halt or power off the system",
        usage: "halt",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "head",
        summary: "print the first lines of input",
        usage: "head [count] [file]",
        source: SBASE,
        tier: TIER0,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "hostname",
        summary: "print or set the host name",
        usage: "hostname [name]",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "hwclock",
        summary: "read or set the hardware clock",
        usage: "hwclock",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "id",
        summary: "print user and group identity",
        usage: "id [user]",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "insmod",
        summary: "insert a kernel module",
        usage: "insmod <module>",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "join",
        summary: "join lines from two sorted files",
        usage: "join <left> <right>",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "kill",
        summary: "send a signal to processes",
        usage: "kill <pid>...",
        source: SBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "killall5",
        summary: "signal all processes outside the caller session",
        usage: "killall5 [signal]",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "last",
        summary: "show recent login sessions",
        usage: "last",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "lastlog",
        summary: "show last-login records",
        usage: "lastlog",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "link",
        summary: "create a hard link",
        usage: "link <target> <link>",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "ln",
        summary: "create hard or symbolic links",
        usage: "ln [-s] <target> <link>",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "logger",
        summary: "write messages to the system log",
        usage: "logger <message>",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "login",
        summary: "authenticate and start a user session",
        usage: "login [user]",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "logname",
        summary: "print the login name",
        usage: "logname",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "ls",
        summary: "list directory entries",
        usage: "ls [path]",
        source: SBASE,
        tier: TIER0,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "lsmod",
        summary: "list loaded kernel modules",
        usage: "lsmod",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "lsusb",
        summary: "list USB devices",
        usage: "lsusb",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "make",
        summary: "execute dependency rules from a makefile",
        usage: "make [target]",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "md5sum",
        summary: "compute MD5 digests",
        usage: "md5sum <file>...",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "mesg",
        summary: "control terminal write permission",
        usage: "mesg [y|n]",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "mkdir",
        summary: "create directories",
        usage: "mkdir <path>...",
        source: SBASE,
        tier: TIER0,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "mkfifo",
        summary: "create named pipes",
        usage: "mkfifo <path>...",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "mknod",
        summary: "create device or special files",
        usage: "mknod <path> <type> <major> <minor>",
        source: SBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "mkswap",
        summary: "initialize a swap area",
        usage: "mkswap <device>",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "mktemp",
        summary: "create a unique temporary path",
        usage: "mktemp [template]",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "mount",
        summary: "attach a filesystem to the mount tree",
        usage: "mount <source> <target>",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "mountpoint",
        summary: "test whether a path is a mount point",
        usage: "mountpoint <path>",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "mv",
        summary: "move or rename paths",
        usage: "mv <source> <target>",
        source: SBASE,
        tier: TIER0,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "nice",
        summary: "run a command with adjusted priority",
        usage: "nice [-n inc] <command>",
        source: SBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "nl",
        summary: "number lines",
        usage: "nl [file]",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "nohup",
        summary: "run a command immune to hangup",
        usage: "nohup <command>",
        source: SBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "nologin",
        summary: "refuse login with a message",
        usage: "nologin",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "od",
        summary: "dump bytes in octal or other formats",
        usage: "od [file]",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "pagesize",
        summary: "print system page size",
        usage: "pagesize",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "passwd",
        summary: "change user password metadata",
        usage: "passwd [user]",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "paste",
        summary: "merge lines from files",
        usage: "paste <file>...",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "pathchk",
        summary: "validate path portability",
        usage: "pathchk <path>...",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "pidof",
        summary: "find process IDs by name",
        usage: "pidof <name>",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "pivot_root",
        summary: "change the root and old-root mount points",
        usage: "pivot_root <new_root> <put_old>",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "printenv",
        summary: "print environment variables",
        usage: "printenv [name]",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "printf",
        summary: "format and print arguments",
        usage: "printf <format> [arg]...",
        source: SBASE,
        tier: TIER0,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "ps",
        summary: "list processes",
        usage: "ps",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "pwd",
        summary: "print the current directory",
        usage: "pwd",
        source: SBASE,
        tier: TIER0,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "pwdx",
        summary: "print process working directories",
        usage: "pwdx <pid>...",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "readahead",
        summary: "preload file contents into cache",
        usage: "readahead <file>...",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "readlink",
        summary: "print symbolic link targets",
        usage: "readlink <path>",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "renice",
        summary: "change process priority",
        usage: "renice <priority> <pid>...",
        source: SBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "respawn",
        summary: "restart a command when it exits",
        usage: "respawn <command>",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "rev",
        summary: "reverse characters in each line",
        usage: "rev [file]",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "rm",
        summary: "remove directory entries",
        usage: "rm <path>...",
        source: SBASE,
        tier: TIER0,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "rmdir",
        summary: "remove empty directories",
        usage: "rmdir <path>...",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "rmmod",
        summary: "remove a kernel module",
        usage: "rmmod <module>",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "sed",
        summary: "edit streams with scripted substitutions",
        usage: "sed <script> [file]",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "seq",
        summary: "print numeric sequences",
        usage: "seq <last>",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "setsid",
        summary: "run a command in a new session",
        usage: "setsid <command>",
        source: SBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "sha1sum",
        summary: "compute SHA-1 digests",
        usage: "sha1sum <file>...",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "sha224sum",
        summary: "compute SHA-224 digests",
        usage: "sha224sum <file>...",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "sha256sum",
        summary: "compute SHA-256 digests",
        usage: "sha256sum <file>...",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "sha384sum",
        summary: "compute SHA-384 digests",
        usage: "sha384sum <file>...",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "sha512-224sum",
        summary: "compute SHA-512/224 digests",
        usage: "sha512-224sum <file>...",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "sha512-256sum",
        summary: "compute SHA-512/256 digests",
        usage: "sha512-256sum <file>...",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "sha512sum",
        summary: "compute SHA-512 digests",
        usage: "sha512sum <file>...",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "sleep",
        summary: "delay for a duration",
        usage: "sleep <seconds>",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "sort",
        summary: "sort input lines",
        usage: "sort [file]",
        source: SBASE,
        tier: TIER0,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "split",
        summary: "split files into pieces",
        usage: "split [file] [prefix]",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "sponge",
        summary: "read all input before writing output",
        usage: "sponge <file>",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "stat",
        summary: "print file metadata",
        usage: "stat <path>",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "strings",
        summary: "print printable byte runs from files",
        usage: "strings <file>...",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "su",
        summary: "switch user identity",
        usage: "su [user]",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "swaplabel",
        summary: "print or set swap area labels",
        usage: "swaplabel <device>",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "swapoff",
        summary: "disable swap devices or files",
        usage: "swapoff <path>",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "swapon",
        summary: "enable swap devices or files",
        usage: "swapon <path>",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "switch_root",
        summary: "switch to another root filesystem",
        usage: "switch_root <new_root> <init>",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "sync",
        summary: "flush filesystem buffers",
        usage: "sync",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "sysctl",
        summary: "read or write kernel parameters",
        usage: "sysctl <name>[=value]",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "tail",
        summary: "print the last lines of input",
        usage: "tail [count] [file]",
        source: SBASE,
        tier: TIER0,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "tar",
        summary: "create or extract tar archives",
        usage: "tar <mode> <archive> [path]...",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "tee",
        summary: "copy stdin to stdout and files",
        usage: "tee <file>...",
        source: SBASE,
        tier: TIER0,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "test",
        summary: "evaluate file, string, and integer predicates",
        usage: "test <expression>",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "tftp",
        summary: "transfer files using TFTP",
        usage: "tftp <host>",
        source: SBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "time",
        summary: "measure command runtime",
        usage: "time <command>",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "touch",
        summary: "create files or update timestamps",
        usage: "touch <path>...",
        source: SBASE,
        tier: TIER0,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "tr",
        summary: "translate or delete characters",
        usage: "tr <set1> <set2>",
        source: SBASE,
        tier: TIER0,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "true",
        summary: "return a successful exit status",
        usage: "true",
        source: SBASE,
        tier: TIER0,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "truncate",
        summary: "change a file length",
        usage: "truncate -s <size> <file>",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "tsort",
        summary: "topologically sort dependency pairs",
        usage: "tsort [file]",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "tty",
        summary: "print the terminal path",
        usage: "tty",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "umount",
        summary: "detach mounted filesystems",
        usage: "umount <target>",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "uname",
        summary: "print system identity",
        usage: "uname [-a]",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "unexpand",
        summary: "convert spaces to tabs",
        usage: "unexpand [file]",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "uniq",
        summary: "remove adjacent duplicate lines",
        usage: "uniq [file]",
        source: SBASE,
        tier: TIER0,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "unlink",
        summary: "remove one directory entry",
        usage: "unlink <path>",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "unshare",
        summary: "run with separated namespaces",
        usage: "unshare <flags> <command>",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "uptime",
        summary: "print system uptime",
        usage: "uptime",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "uudecode",
        summary: "decode uuencoded files",
        usage: "uudecode [file]",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "uuencode",
        summary: "encode files using uuencode",
        usage: "uuencode <file> <name>",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "vtallow",
        summary: "allow or deny virtual-terminal switching",
        usage: "vtallow <yes|no>",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "watch",
        summary: "repeat a command periodically",
        usage: "watch <command>",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "wc",
        summary: "count lines, words, and characters",
        usage: "wc [file]",
        source: SBASE,
        tier: TIER0,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "which",
        summary: "locate commands in the command catalog",
        usage: "which <name>",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "who",
        summary: "show logged-in users",
        usage: "who",
        source: UBASE,
        tier: TIER2,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "whoami",
        summary: "print current user name",
        usage: "whoami",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "xargs",
        summary: "build command arguments from stdin",
        usage: "xargs <command>",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "xinstall",
        summary: "copy files while setting metadata",
        usage: "xinstall <source> <target>",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
    CommandDescriptor {
        name: "yes",
        summary: "repeat a string until stopped",
        usage: "yes [text]",
        source: SBASE,
        tier: TIER1,
        state: BRIDGE,
    },
];

pub fn commands() -> &'static [CommandDescriptor] {
    COMMANDS
}

pub fn lookup(name: &str) -> Option<&'static CommandDescriptor> {
    COMMANDS.iter().find(|command| command.name == name)
}

pub fn tier_count(tier: CommandTier) -> usize {
    COMMANDS
        .iter()
        .filter(|command| command.tier == tier)
        .count()
}

pub fn state_count(state: CommandState) -> usize {
    COMMANDS
        .iter()
        .filter(|command| command.state == state)
        .count()
}

pub fn dispatch<'a>(argv: &'a [&'a str]) -> Dispatch<'a> {
    let Some(command) = argv.first().copied() else {
        return Dispatch::List;
    };

    if command == "help" {
        return Dispatch::Help(argv.get(1).and_then(|name| lookup(name)));
    }

    let Some(descriptor) = lookup(command) else {
        return Dispatch::Unknown(command);
    };

    match descriptor.state {
        CommandState::ShellBridge => Dispatch::RunShellBridge {
            descriptor,
            args: &argv[1..],
        },
        CommandState::AdapterPending => Dispatch::AdapterPending(descriptor),
    }
}

pub fn render_catalog() -> String {
    let mut out = String::from("ech-tools command catalog\n");
    out.push_str("=========================\n");
    out.push_str("unique source commands: 150\n");
    out.push_str("routed through shell bridge: ");
    out.push_str(alloc::format!("{}", state_count(CommandState::ShellBridge)).as_str());
    out.push('\n');
    out.push_str("adapter pending: ");
    out.push_str(alloc::format!("{}", state_count(CommandState::AdapterPending)).as_str());
    out.push_str("\n\n");

    for command in COMMANDS {
        out.push_str(command.name);
        out.push_str(" [");
        out.push_str(command.tier.as_str());
        out.push('/');
        out.push_str(command.state.as_str());
        out.push_str("] - ");
        out.push_str(command.summary);
        out.push('\n');
    }

    out.trim_end().into()
}

pub fn render_detail(command: &CommandDescriptor) -> String {
    alloc::format!(
        "{}\n  usage: {}\n  source: {}\n  tier: {}\n  state: {}\n  summary: {}",
        command.name,
        command.usage,
        command.source.as_str(),
        command.tier.as_str(),
        command.state.as_str(),
        command.summary,
    )
}

pub fn render_unknown(name: &str) -> String {
    alloc::format!(
        "ech-tools: '{}' katalogda yok\nKullanim: ech-tools <komut> [argumanlar]\nListe: ech-tools",
        name
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::BTreeSet;

    #[test]
    fn catalog_matches_unique_source_count() {
        assert_eq!(commands().len(), SOURCE_COMMANDS_UNIQUE);
        assert_eq!(
            SOURCE_COMMANDS_TOTAL - SOURCE_DUPLICATES.len(),
            SOURCE_COMMANDS_UNIQUE
        );
    }

    #[test]
    fn catalog_names_are_unique_and_sorted() {
        let mut names = BTreeSet::new();
        let mut previous = "";
        for command in commands() {
            assert!(names.insert(command.name));
            assert!(previous <= command.name);
            previous = command.name;
        }
    }

    #[test]
    fn tier0_and_system_pool_have_shell_bridge_commands() {
        assert_eq!(tier_count(CommandTier::Tier0), 22);
        assert_eq!(state_count(CommandState::ShellBridge), 150);
        assert_eq!(state_count(CommandState::AdapterPending), 0);
        assert_eq!(lookup("ls").unwrap().state, CommandState::ShellBridge);
        assert_eq!(lookup("tr").unwrap().state, CommandState::ShellBridge);
        assert_eq!(lookup("seq").unwrap().state, CommandState::ShellBridge);
        assert_eq!(lookup("stat").unwrap().state, CommandState::ShellBridge);
        assert_eq!(lookup("cal").unwrap().state, CommandState::ShellBridge);
        assert_eq!(lookup("printenv").unwrap().state, CommandState::ShellBridge);
        assert_eq!(
            lookup("sha256sum").unwrap().state,
            CommandState::ShellBridge
        );
        assert_eq!(lookup("xargs").unwrap().state, CommandState::ShellBridge);
        assert_eq!(lookup("chroot").unwrap().state, CommandState::ShellBridge);
        assert_eq!(
            lookup("switch_root").unwrap().state,
            CommandState::ShellBridge
        );
    }

    #[test]
    fn dispatcher_routes_shell_bridge_commands() {
        let argv = ["echo", "hello"];
        let result = dispatch(&argv);
        assert!(matches!(result, Dispatch::RunShellBridge { .. }));
        if let Dispatch::RunShellBridge { descriptor, args } = result {
            assert_eq!(descriptor.name, "echo");
            assert_eq!(args, &["hello"]);
        }
    }

    #[test]
    fn dispatcher_routes_former_adapter_commands() {
        let argv = ["chroot", "/newroot"];
        let result = dispatch(&argv);
        assert!(matches!(result, Dispatch::RunShellBridge { .. }));
        if let Dispatch::RunShellBridge { descriptor, args } = result {
            assert_eq!(descriptor.name, "chroot");
            assert_eq!(args, &["/newroot"]);
        }
    }
}
