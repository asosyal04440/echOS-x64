use super::super::super::fs;
use super::super::super::gui::launch_pipeline::{
    DpiVirtualizationMode, ExternalDisplayContract, LaunchSession,
};
use super::super::super::posix::{self, WindowsRuntimeError};
use super::super::super::runtime_layer::launch_contract::{self, IsolationDomain};
use super::super::super::runtime_layer::package_registry_contract::RuntimePackageRegistry;
use super::super::super::task::task::Priority;
use super::*;

impl DesktopSession {
    pub(super) fn activate_app(&mut self, kind: AppKind) -> Result<(), String> {
        let policy = match kind {
            AppKind::Terminal => self.resolve_window_launch_policy(
                self.terminal.client.app_id(),
                "Terminal",
                self.terminal.workspace_id,
                Rect::new(self.screen.x + 86, self.screen.y + 126, 720, 420),
                true,
            ),
            AppKind::Files => self.resolve_window_launch_policy(
                self.files.client.app_id(),
                "Files",
                self.files.workspace_id,
                Rect::new(self.screen.x + 232, self.screen.y + 168, 580, 360),
                true,
            ),
            AppKind::Browser => self.resolve_window_launch_policy(
                self.browser.client.app_id(),
                "Web",
                self.browser.workspace_id,
                Rect::new(self.screen.x + 196, self.screen.y + 118, 820, 520),
                true,
            ),
            AppKind::Settings => self.resolve_window_launch_policy(
                self.settings.client.app_id(),
                "Settings",
                self.settings.workspace_id,
                Rect::new(self.screen.x + 244, self.screen.y + 152, 540, 460),
                true,
            ),
            AppKind::Editor => self.resolve_window_launch_policy(
                self.editor.client.app_id(),
                "Editor",
                self.editor.workspace_id,
                Rect::new(self.screen.x + 172, self.screen.y + 110, 760, 500),
                true,
            ),
        };
        let has_window = match kind {
            AppKind::Terminal => self.terminal.window.is_some(),
            AppKind::Files => self.files.window.is_some(),
            AppKind::Browser => self.browser.window.is_some(),
            AppKind::Settings => self.settings.window.is_some(),
            AppKind::Editor => self.editor.window.is_some(),
        };
        let target_workspace = policy.workspace_id;
        if has_window && target_workspace != self.shell.active_workspace {
            self.switch_workspace(target_workspace)?;
        }

        let outcome = match kind {
            AppKind::Terminal => {
                self.terminal.workspace_id = policy.workspace_id;
                self.terminal.ensure_window(self.screen, &policy)?
            }
            AppKind::Files => {
                self.files.workspace_id = policy.workspace_id;
                self.files.ensure_window(self.screen, &policy)?
            }
            AppKind::Browser => {
                self.browser.workspace_id = policy.workspace_id;
                self.browser.ensure_window(self.screen, &policy)?
            }
            AppKind::Settings => {
                self.settings.workspace_id = policy.workspace_id;
                self.settings.ensure_window(self.screen, &policy)?
            }
            AppKind::Editor => {
                self.editor.workspace_id = policy.workspace_id;
                self.editor.ensure_window(self.screen, &policy)?
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

    pub(super) fn launch_registry(&self) -> [PackageRecord; 19] {
        desktop_launch_registry()
    }

    pub(super) fn launch_resolution(&self, query: &str) -> Option<AppResolution> {
        let registry = self.launch_registry();
        peek_launch_resolution(&registry, query)
    }

    pub(super) fn ensure_launch_resolution(&mut self, query: &str) -> Option<AppResolution> {
        let registry = self.launch_registry();
        resolve_launch_resolution_with_seed_install(&registry, query)
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
        let session = self.decorate_external_launch_session(session);
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

    fn decorate_external_launch_session(&self, session: LaunchSession) -> LaunchSession {
        match session.process.bootstrap {
            RuntimeBootstrap::Win32Bridge | RuntimeBootstrap::PosixBridge => {
                session.with_external_display_contract(self.external_display_contract_for(&session))
            }
            _ => session,
        }
    }

    fn external_display_contract_for(&self, session: &LaunchSession) -> ExternalDisplayContract {
        let profile = self.display_profile();
        let accessibility = self
            .shell
            .client
            .accessibility_profile()
            .unwrap_or_default();
        let output_id = super::output_for_workspace(&profile, session.window.workspace_id);
        let output = profile
            .outputs
            .iter()
            .find(|output| output.output_id == output_id)
            .or_else(|| profile.outputs.first());
        let Some(output) = output else {
            return ExternalDisplayContract::default();
        };
        let text_scale_100x = output
            .text_scale_100x
            .max(accessibility.text_scale_100x.max(75));
        let dpi_virtualization = match session.process.abi {
            AbiPersonality::Win32 => {
                if output.scale_100x != 100 || text_scale_100x != 100 {
                    DpiVirtualizationMode::BitmapScale
                } else {
                    DpiVirtualizationMode::SystemAware
                }
            }
            AbiPersonality::Posix => DpiVirtualizationMode::Native,
            AbiPersonality::Native => DpiVirtualizationMode::Native,
        };
        ExternalDisplayContract {
            output_id: output.output_id,
            ui_scale_100x: output.scale_100x.max(100),
            text_scale_100x,
            cursor_scale_100x: accessibility.cursor_scale_100x.max(100),
            dpi_virtualization,
        }
    }
}

pub(super) fn launch_path_exists(path: &str) -> bool {
    fs::vfs_open_inode(path).is_ok()
}

pub(super) fn peek_launch_resolution(
    registry: &[PackageRecord],
    query: &str,
) -> Option<AppResolution> {
    RuntimePackageRegistry::new(registry).resolve_with_probe(query, launch_path_exists)
}

pub(super) fn resolve_launch_resolution_with_seed_install(
    registry: &[PackageRecord],
    query: &str,
) -> Option<AppResolution> {
    let first = peek_launch_resolution(registry, query);
    let seed_query = match first.as_ref() {
        Some(AppResolution::MissingExternalPath { descriptor, .. }) => Some(descriptor.package_id),
        Some(_) => None,
        None => Some(query),
    };
    let Some(seed_query) = seed_query else {
        return first;
    };
    match crate::security::seed_store::install_seed_for_query(seed_query) {
        Ok(crate::security::seed_store::SeedInstallOutcome::Installed)
        | Ok(crate::security::seed_store::SeedInstallOutcome::Updated)
        | Ok(crate::security::seed_store::SeedInstallOutcome::AlreadyInstalled) => {
            peek_launch_resolution(registry, query)
        }
        Ok(crate::security::seed_store::SeedInstallOutcome::NotFound) => first,
        Err(err) => {
            crate::serial_println!(
                "[SEED] on-demand install fail query={} err={}",
                seed_query,
                err
            );
            first
        }
    }
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
