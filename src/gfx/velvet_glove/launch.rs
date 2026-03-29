use super::super::super::fs;
use super::super::super::posix::{self, WindowsRuntimeError};
use super::super::super::runtime_layer::launch_contract::{self, IsolationDomain};
use super::super::super::runtime_layer::package_registry_contract::RuntimePackageRegistry;
use super::super::super::task::task::Priority;
use super::*;

impl DesktopSession {
    pub(super) fn activate_app(&mut self, kind: AppKind) -> Result<(), String> {
        let has_window = match kind {
            AppKind::Terminal => self.terminal.window.is_some(),
            AppKind::Files => self.files.window.is_some(),
            AppKind::Browser => self.browser.window.is_some(),
            AppKind::Settings => self.settings.window.is_some(),
            AppKind::Editor => self.editor.window.is_some(),
        };
        let target_workspace = self.app_workspace(kind);
        if has_window && target_workspace != self.shell.active_workspace {
            self.switch_workspace(target_workspace)?;
        }

        let outcome = match kind {
            AppKind::Terminal => {
                if self.terminal.window.is_none() {
                    self.terminal.workspace_id = self.shell.active_workspace;
                }
                self.terminal.ensure_window(self.screen)?
            }
            AppKind::Files => {
                if self.files.window.is_none() {
                    self.files.workspace_id = self.shell.active_workspace;
                }
                self.files.ensure_window(self.screen)?
            }
            AppKind::Browser => {
                if self.browser.window.is_none() {
                    self.browser.workspace_id = self.shell.active_workspace;
                }
                self.browser.ensure_window(self.screen)?
            }
            AppKind::Settings => {
                if self.settings.window.is_none() {
                    self.settings.workspace_id = self.shell.active_workspace;
                }
                self.settings.ensure_window(self.screen)?
            }
            AppKind::Editor => {
                if self.editor.window.is_none() {
                    self.editor.workspace_id = self.shell.active_workspace;
                }
                self.editor.ensure_window(self.screen)?
            }
        };

        let verb = match outcome {
            LaunchResult::Launched => "launched",
            LaunchResult::Restored => "restored",
            LaunchResult::Focused => "focused",
        };
        let _ = self.relayout_workspace(self.shell.active_workspace);
        self.rebuild_stage_sets();
        self.push_notice(format!("{} {}", kind.title(), verb));
        self.mark_shell_dirty();
        Ok(())
    }

    pub(super) fn open_files_path(&mut self, path: &str) -> Result<(), String> {
        self.files.current_path = String::from(path);
        if self.files.window.is_some() {
            self.files.refresh()?;
        }
        self.activate_app(AppKind::Files)
    }

    pub(super) fn open_settings_hub(&mut self, detail: &str) -> Result<(), String> {
        self.settings.dirty = true;
        self.activate_app(AppKind::Settings)?;
        self.push_notice(String::from(detail));
        Ok(())
    }

    pub(super) fn launch_intent_for_app(
        &self,
        kind: AppKind,
        source: LaunchSource,
    ) -> LaunchIntent {
        LaunchIntent::new(
            kind.descriptor(),
            ExecutionContext::new(source, self.shell.active_workspace, kind.title()),
        )
    }

    pub(super) fn launch_intent_for_shortcut(
        &self,
        shortcut: DesktopShortcutKind,
        source: LaunchSource,
    ) -> LaunchIntent {
        LaunchIntent::new(
            shortcut.descriptor(),
            ExecutionContext::new(source, self.shell.active_workspace, shortcut.route_label()),
        )
    }

    pub(super) fn launch_registry(&self) -> [PackageRecord; 9] {
        desktop_launch_registry()
    }

    pub(super) fn launch_resolution(&self, query: &str) -> Option<AppResolution> {
        let registry = self.launch_registry();
        RuntimePackageRegistry::new(&registry).resolve_with_probe(query, launch_path_exists)
    }

    pub(super) fn evaluate_launch_policy(&self, intent: &LaunchIntent) -> Result<(), String> {
        for permission in intent
            .descriptor
            .capabilities
            .permissions()
            .into_iter()
            .flatten()
        {
            if self.shell.client.permission_state(permission)? == PermissionState::Denied {
                return Err(format!(
                    "policy gate blocked {}: {:?} denied",
                    intent.descriptor.title, permission
                ));
            }
        }
        Ok(())
    }

    pub(super) fn dispatch_launch_session(
        &mut self,
        session: LaunchSession,
        path: Option<&str>,
    ) -> Result<(), String> {
        match session.process.bootstrap {
            RuntimeBootstrap::NativeWindowed => {
                if let Some(path) = path {
                    let title = session.intent.descriptor.title;
                    let task_name = session.intent.descriptor.slug;
                    let runtime = launch_contract::spawn_native_runtime(
                        session,
                        Priority::Normal,
                        task_name,
                        path,
                    )?;
                    self.push_notice(format!(
                        "{} launched through Native SDK runtime (rt#{})",
                        title, runtime.id
                    ));
                    return Ok(());
                }
                let _ = launch_contract::register_launch_session(
                    session,
                    None,
                    None,
                    None,
                    IsolationDomain::KernelTask,
                    None,
                );
                let Some(kind) = app_kind_from_id(session.window.app_id) else {
                    return Err(format!(
                        "launch resolution lost native descriptor for app {}",
                        session.intent.descriptor.title
                    ));
                };
                if !self.app_has_window(kind)
                    && self.shell.active_workspace != session.window.workspace_id
                {
                    self.switch_workspace(session.window.workspace_id)?;
                }
                self.activate_app(kind)
            }
            RuntimeBootstrap::NativeSpecialAction => match session.window.app_id {
                RECYCLE_SHORTCUT_APP_ID => {
                    let _ = launch_contract::register_launch_session(
                        session,
                        None,
                        None,
                        None,
                        IsolationDomain::KernelTask,
                        None,
                    );
                    self.push_notice(String::from("Recycle Bin is empty"));
                    Ok(())
                }
                app_id => Err(format!("native special action {} not resolved", app_id)),
            },
            RuntimeBootstrap::NativeHeadless => Err(String::from(
                "headless runtime cannot be launched from desktop surface",
            )),
            RuntimeBootstrap::Win32Bridge => {
                let path = path.ok_or_else(|| String::from("PE launch path missing"))?;
                let image = load_launch_image(path)?;
                posix::prepare_windows_launch(&image).map_err(format_windows_launch_error)?;
                let runtime = launch_contract::spawn_pe_runtime(
                    session,
                    &image,
                    Priority::Normal,
                    "pe-runtime",
                    Some(path),
                )?;
                self.push_notice(format!(
                    "{} routed through Win32 bridge (rt#{})",
                    path, runtime.id
                ));
                Ok(())
            }
            RuntimeBootstrap::PosixBridge => {
                let path = path.ok_or_else(|| String::from("ELF launch path missing"))?;
                let image = load_launch_image(path)?;
                let runtime = launch_contract::spawn_elf_runtime(
                    session,
                    &image,
                    Priority::Normal,
                    "elf-runtime",
                    Some(path),
                )?;
                self.push_notice(format!(
                    "{} routed through POSIX bridge (rt#{})",
                    path, runtime.id
                ));
                Ok(())
            }
        }
    }

    pub(super) fn dispatch_launch_intent(&mut self, intent: LaunchIntent) -> Result<(), String> {
        self.evaluate_launch_policy(&intent)?;
        self.dispatch_launch_session(intent.canonical_session(), None)
    }

    pub(super) fn dispatch_external_launch_intent(
        &mut self,
        intent: LaunchIntent,
        path: &str,
    ) -> Result<(), String> {
        self.evaluate_launch_policy(&intent)?;
        self.dispatch_launch_session(intent.canonical_session(), Some(path))
    }
}

pub(super) fn launch_path_exists(path: &str) -> bool {
    fs::vfs_open_inode(path).is_ok()
}

fn load_launch_image(path: &str) -> Result<Vec<u8>, String> {
    let inode = fs::vfs_open_inode(path).map_err(|_| String::from("Dosya bulunamadi"))?;
    let size = fs::vfs_inode_metadata(&inode)
        .map_err(|_| String::from("Dosya bilgisi okunamadi"))?
        .size;
    let mut data = vec![0u8; size];
    let mut offset = 0usize;
    while offset < data.len() {
        let read = fs::vfs_read_at(&inode, offset, &mut data[offset..])
            .map_err(|_| String::from("Dosya okunamadi"))?;
        if read == 0 {
            break;
        }
        offset += read;
    }
    data.truncate(offset);
    if data.is_empty() {
        return Err(String::from("Goruntu bos veya okunamadi"));
    }
    Ok(data)
}

fn format_windows_launch_error(err: WindowsRuntimeError) -> String {
    match err {
        WindowsRuntimeError::NotFound => String::from("Runtime secilmedi"),
        WindowsRuntimeError::Invalid => String::from("Gecersiz hedef"),
        WindowsRuntimeError::SecureBootViolation => {
            String::from("Secure Boot aktif, imzasiz PE reddedildi")
        }
    }
}
