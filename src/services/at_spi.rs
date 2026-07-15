//! Minimal AT-SPI bridge for accessibility tree publication.

use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

use crate::gui::protocol::{AccessibilityNode, AccessibilityRole, AppId, Rect};

#[derive(Clone, Debug)]
pub struct AtSpiEvent {
    pub app_id: AppId,
    pub node_id: u64,
    pub role: AccessibilityRole,
    pub label: String,
    pub focused: bool,
    pub bounds: Rect,
}

pub struct AtSpiBridge {
    running: AtomicBool,
    trees: Mutex<BTreeMap<AppId, Vec<AccessibilityNode>>>,
    events: Mutex<VecDeque<AtSpiEvent>>,
}

impl AtSpiBridge {
    pub fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            trees: Mutex::new(BTreeMap::new()),
            events: Mutex::new(VecDeque::new()),
        }
    }

    pub fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
    }

    pub fn publish_tree(&self, app_id: AppId, nodes: &[AccessibilityNode]) {
        if !self.running.load(Ordering::Relaxed) {
            return;
        }

        self.trees.lock().insert(app_id, nodes.to_vec());
        let mut events = self.events.lock();
        for node in nodes {
            events.push_back(AtSpiEvent {
                app_id,
                node_id: node.id,
                role: node.role,
                label: node.label.clone(),
                focused: node.focused,
                bounds: node.bounds,
            });
        }
    }

    pub fn pull_events(&self, max: usize) -> Vec<AtSpiEvent> {
        let mut out = Vec::new();
        let mut events = self.events.lock();
        for _ in 0..max {
            let Some(evt) = events.pop_front() else {
                break;
            };
            out.push(evt);
        }
        out
    }

    pub fn get_tree(&self, app_id: AppId) -> Vec<AccessibilityNode> {
        self.trees.lock().get(&app_id).cloned().unwrap_or_default()
    }
}

static AT_SPI: spin::Lazy<Arc<AtSpiBridge>> = spin::Lazy::new(|| Arc::new(AtSpiBridge::new()));

pub fn init() {
    AT_SPI.start();
    crate::serial_println!("[AT-SPI] Bridge initialized");
}

pub fn get_bridge() -> Arc<AtSpiBridge> {
    Arc::clone(&AT_SPI)
}
