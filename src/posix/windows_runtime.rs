use alloc::string::{String, ToString};
use alloc::vec::Vec;

use spin::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowsRuntimeFlavor {
    DesktopCompat,
    GameCompat,
}

#[derive(Clone, Debug)]
pub struct WindowsRuntime {
    pub name: String,
    pub root_path: String,
    pub flavor: WindowsRuntimeFlavor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowsRuntimeError {
    NotFound,
    Invalid,
    SecureBootViolation,
}

static WINDOWS_RUNTIME_REGISTRY: Mutex<Vec<WindowsRuntime>> = Mutex::new(Vec::new());
static WINDOWS_RUNTIME_ACTIVE: Mutex<Option<usize>> = Mutex::new(None);

pub fn upsert_windows_runtime(
    name: &str,
    root_path: &str,
    flavor: WindowsRuntimeFlavor,
) -> Result<usize, WindowsRuntimeError> {
    if name.trim().is_empty() || root_path.trim().is_empty() {
        return Err(WindowsRuntimeError::Invalid);
    }
    let mut registry = WINDOWS_RUNTIME_REGISTRY.lock();
    if let Some((idx, runtime)) = registry
        .iter_mut()
        .enumerate()
        .find(|(_, runtime)| runtime.name == name)
    {
        runtime.root_path = root_path.to_string();
        runtime.flavor = flavor;
        *WINDOWS_RUNTIME_ACTIVE.lock() = Some(idx);
        return Ok(idx);
    }
    registry.push(WindowsRuntime {
        name: name.to_string(),
        root_path: root_path.to_string(),
        flavor,
    });
    let idx = registry.len() - 1;
    *WINDOWS_RUNTIME_ACTIVE.lock() = Some(idx);
    Ok(idx)
}

pub fn list_windows_runtimes() -> Vec<WindowsRuntime> {
    WINDOWS_RUNTIME_REGISTRY.lock().clone()
}

pub fn select_windows_runtime(name: &str) -> Result<(), WindowsRuntimeError> {
    let idx = WINDOWS_RUNTIME_REGISTRY
        .lock()
        .iter()
        .position(|runtime| runtime.name == name)
        .ok_or(WindowsRuntimeError::NotFound)?;
    *WINDOWS_RUNTIME_ACTIVE.lock() = Some(idx);
    Ok(())
}

pub fn current_windows_runtime() -> Option<WindowsRuntime> {
    let idx = *WINDOWS_RUNTIME_ACTIVE.lock();
    let registry = WINDOWS_RUNTIME_REGISTRY.lock();
    idx.and_then(|value| registry.get(value).cloned())
}

pub fn run_windows_app(path: &str) -> Result<(), WindowsRuntimeError> {
    super::windows_image::run_windows_app(path)
}
