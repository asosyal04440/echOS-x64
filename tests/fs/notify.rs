//! # Wave 5.9.5 — Notify Corpus
//!
//! Host-side simulation of inotify-style filesystem notification:
//! create/delete events, rename cookies, overflow, coalescing, inode deletion.

#![cfg(not(target_os = "none"))]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

static SEQ_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_seq() -> u64 {
    SEQ_COUNTER.fetch_add(1, Ordering::Relaxed)
}

fn next_cookie() -> u32 {
    static COOKIE: AtomicU32 = AtomicU32::new(1);
    loop {
        let val = COOKIE.fetch_add(1, Ordering::Relaxed);
        if val != 0 {
            return val;
        }
    }
}

const IN_CREATE: u32 = 0x00000100;
const IN_DELETE: u32 = 0x00000200;
const IN_MOVED_FROM: u32 = 0x00000040;
const IN_MOVED_TO: u32 = 0x00000080;
const IN_Q_OVERFLOW: u32 = 0x00004000;
const IN_IGNORED: u32 = 0x00008000;
const IN_ISDIR: u32 = 0x40000000;
const IN_MODIFY: u32 = 0x00000002;

#[derive(Debug, Clone)]
struct NotifyEvent {
    seq_no: u64,
    wd: i32,
    mask: u32,
    cookie: u32,
    name: String,
}

impl NotifyEvent {
    fn new(wd: i32, mask: u32, cookie: u32, name: &str) -> Self {
        Self {
            seq_no: next_seq(),
            wd,
            mask,
            cookie,
            name: name.to_string(),
        }
    }
}

struct NotifyQueue {
    events: VecDeque<NotifyEvent>,
    max_size: usize,
    overflow_emitted: bool,
}

impl NotifyQueue {
    fn new(max_size: usize) -> Self {
        Self {
            events: VecDeque::with_capacity(max_size),
            max_size,
            overflow_emitted: false,
        }
    }

    fn push(&mut self, event: NotifyEvent) -> bool {
        if self.events.len() >= self.max_size {
            if !self.overflow_emitted {
                self.overflow_emitted = true;
                let overflow = NotifyEvent::new(-1, IN_Q_OVERFLOW, 0, "");
                self.events.push_back(overflow);
            }
            self.events.pop_front();
            return false;
        }
        self.events.push_back(event);
        true
    }

    fn coalesce_push(&mut self, event: NotifyEvent) {
        if let Some(last) = self.events.back() {
            if last.wd == event.wd
                && last.mask == event.mask
                && last.name == event.name
            {
                return;
            }
        }
        self.push(event);
    }

    fn drain(&mut self) -> Vec<NotifyEvent> {
        self.events.drain(..).collect()
    }
}

struct WatchTable {
    watches: std::collections::HashMap<i32, WatchEntry>,
    next_wd: i32,
}

struct WatchEntry {
    inode: u64,
    path: String,
    mask: u32,
    active: bool,
}

impl WatchTable {
    fn new() -> Self {
        Self {
            watches: std::collections::HashMap::new(),
            next_wd: 1,
        }
    }

    fn add_watch(&mut self, inode: u64, path: &str, mask: u32) -> i32 {
        let wd = self.next_wd;
        self.next_wd += 1;
        self.watches.insert(
            wd,
            WatchEntry {
                inode,
                path: path.to_string(),
                mask,
                active: true,
            },
        );
        wd
    }

    fn remove_watch(&mut self, wd: i32) -> Option<WatchEntry> {
        self.watches.remove(&wd)
    }

    fn find_by_inode(&self, inode: u64) -> Vec<(i32, &WatchEntry)> {
        self.watches
            .iter()
            .filter(|(_, w)| w.inode == inode && w.active)
            .map(|(wd, w)| (*wd, w))
            .collect()
    }
}

#[test]
fn create_delete() {
    let mut queue = NotifyQueue::new(1024);
    let mut watches = WatchTable::new();

    let wd = watches.add_watch(100, "/watched_dir", IN_CREATE | IN_DELETE);

    let create_ev = NotifyEvent::new(wd, IN_CREATE, 0, "new_file.txt");
    queue.push(create_ev);

    let delete_ev = NotifyEvent::new(wd, IN_DELETE, 0, "new_file.txt");
    queue.push(delete_ev);

    let events = queue.drain();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].mask, IN_CREATE);
    assert_eq!(events[0].name, "new_file.txt");
    assert_eq!(events[1].mask, IN_DELETE);
    assert_eq!(events[1].name, "new_file.txt");
}

#[test]
fn rename_cookie() {
    let mut queue = NotifyQueue::new(1024);
    let mut watches = WatchTable::new();

    let wd_old = watches.add_watch(100, "/old_dir", IN_MOVED_FROM);
    let wd_new = watches.add_watch(200, "/new_dir", IN_MOVED_TO);

    let cookie = next_cookie();

    let moved_from = NotifyEvent::new(wd_old, IN_MOVED_FROM, cookie, "file.txt");
    let moved_to = NotifyEvent::new(wd_new, IN_MOVED_TO, cookie, "file.txt");

    queue.push(moved_from);
    queue.push(moved_to);

    let events = queue.drain();
    assert_eq!(events.len(), 2);
    assert_ne!(events[0].cookie, 0);
    assert_eq!(events[0].cookie, events[1].cookie);
    assert_eq!(events[0].mask, IN_MOVED_FROM);
    assert_eq!(events[1].mask, IN_MOVED_TO);
}

#[test]
fn overflow() {
    let max = 5;
    let mut queue = NotifyQueue::new(max);

    for i in 0..10 {
        let ev = NotifyEvent::new(1, IN_CREATE, 0, &format!("file{}.txt", i));
        queue.push(ev);
    }

    let events = queue.drain();
    assert!(events.iter().any(|e| e.mask == IN_Q_OVERFLOW));
    assert_eq!(events.len(), max);
}

#[test]
fn coalesce() {
    let mut queue = NotifyQueue::new(1024);

    for _ in 0..5 {
        let ev = NotifyEvent::new(1, IN_MODIFY, 0, "same_file.txt");
        queue.coalesce_push(ev);
    }

    let events = queue.drain();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].mask, IN_MODIFY);
    assert_eq!(events[0].name, "same_file.txt");
}

#[test]
fn inode_deletion() {
    let mut queue = NotifyQueue::new(1024);
    let mut watches = WatchTable::new();

    let target_inode = 42u64;
    let wd1 = watches.add_watch(target_inode, "/watched1", IN_DELETE | IN_DELETE);
    let wd2 = watches.add_watch(target_inode, "/watched2", IN_DELETE);

    let deleted_watches = watches.find_by_inode(target_inode);
    let cookies: Vec<i32> = deleted_watches.iter().map(|(wd, _)| *wd).collect();

    for wd in &cookies {
        watches.remove_watch(*wd);
        let ignored = NotifyEvent::new(*wd, IN_IGNORED, 0, "");
        queue.push(ignored);
    }

    let events = queue.drain();
    assert_eq!(events.len(), 2);
    for ev in &events {
        assert_eq!(ev.mask, IN_IGNORED);
    }
    let wd_set: std::collections::HashSet<i32> = events.iter().map(|e| e.wd).collect();
    assert!(wd_set.contains(&wd1));
    assert!(wd_set.contains(&wd2));
}
