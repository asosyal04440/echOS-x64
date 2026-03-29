use super::*;
use crate::runtime_layer::{dialog_client_contract, shell_client_contract};

pub(super) fn session_snapshot_or_fallback(
    session: &DesktopSession,
    overview_active: bool,
    output_scale: u16,
    text_scale: u16,
    shell_state: ShellState,
) -> SessionSnapshot {
    shell_client_contract::session_snapshot().unwrap_or(SessionSnapshot {
        workspace_id: session.shell.active_workspace,
        workspace_layout: WorkspaceLayout::Dwindle,
        power_state: SessionPowerState::Active,
        unread_notifications: 0,
        apps_running: 0,
        apps_crashed: 0,
        overview_active,
        scratchpad_visible: false,
        shell_ready: true,
        boot_clean_desktop: true,
        output_scale: output_scale as u32,
        text_scale: text_scale as u32,
        clipboard_history_len: 0,
        accessibility_profile: AccessibilityProfile::default(),
        display_profile: DisplayProfile::default(),
        shell_density: ShellDensityProfile::Balanced,
        motion_profile: MotionProfile::Standard,
        restore_state: RestoreDisposition::RestoreIfClean,
        stage_set_policy: crate::gui::protocol::StageSetPolicy::default(),
        locale: String::from("en-US"),
        theme_variant: String::from("hybrid-titan"),
        shell_state,
    })
}

impl DesktopSession {
    pub(super) fn enforce_desktop_visibility_contract(&mut self) {
        let Ok(snapshot) = self.shell.client.session_snapshot() else {
            return;
        };

        if !desktop_visibility_recovery_needed(
            snapshot.power_state,
            self.shell.logged_in,
            self.shell.lock_screen.visible,
            self.shell.top_bar.visible,
            self.shell.task_strip.visible,
            self.appliance_auto_login_pending,
            self.desktop_ready_published,
        ) {
            return;
        }

        self.shell.logged_in = true;
        self.set_login_visibility(false);
        let _ = self.relayout_active_workspace();
        let _ = apply_workspace_visibility(
            &self.terminal.client,
            &mut self.terminal.window,
            self.terminal.workspace_id,
            self.shell.active_workspace,
        );
        let _ = apply_workspace_visibility(
            &self.files.client,
            &mut self.files.window,
            self.files.workspace_id,
            self.shell.active_workspace,
        );
        let _ = apply_workspace_visibility(
            &self.browser.client,
            &mut self.browser.window,
            self.browser.workspace_id,
            self.shell.active_workspace,
        );
        let _ = apply_workspace_visibility(
            &self.settings.client,
            &mut self.settings.window,
            self.settings.workspace_id,
            self.shell.active_workspace,
        );
        let _ = apply_workspace_visibility(
            &self.editor.client,
            &mut self.editor.window,
            self.editor.workspace_id,
            self.shell.active_workspace,
        );
        let _ = self.shell.client.focus_window(self.shell.top_bar.window_id);
        self.mark_shell_dirty();
    }

    pub(super) fn run_appliance_auto_login(&mut self) {
        if !self.appliance_auto_login_pending
            || self.shell.logged_in
            || !self.shell.lock_screen.visible
        {
            return;
        }

        self.appliance_auto_login_pending = false;
        self.shell.auth_input.clear();
        self.shell.auth_input.push_str("echos");
        self.invalidate_shell(
            InvalidationTarget::LockScreen,
            InvalidationReason::StateChanged,
        );
        crate::serial_println!("[DESKTOP] appliance auto-login attempt");
        self.attempt_login();
    }

    pub(super) fn is_locked(&self) -> bool {
        matches!(
            self.shell.client.session_snapshot(),
            Ok(snapshot) if snapshot.power_state == SessionPowerState::Locked
        ) || !self.shell.logged_in
    }

    pub(super) fn unlock_session(&mut self) {
        self.shell.logged_in = true;
        let _ = self.shell.client.set_power_state(SessionPowerState::Active);
        self.set_login_visibility(false);
        let _ = apply_workspace_visibility(
            &self.terminal.client,
            &mut self.terminal.window,
            self.terminal.workspace_id,
            self.shell.active_workspace,
        );
        let _ = apply_workspace_visibility(
            &self.files.client,
            &mut self.files.window,
            self.files.workspace_id,
            self.shell.active_workspace,
        );
        let _ = apply_workspace_visibility(
            &self.browser.client,
            &mut self.browser.window,
            self.browser.workspace_id,
            self.shell.active_workspace,
        );
        let _ = apply_workspace_visibility(
            &self.settings.client,
            &mut self.settings.window,
            self.settings.workspace_id,
            self.shell.active_workspace,
        );
        let _ = apply_workspace_visibility(
            &self.editor.client,
            &mut self.editor.window,
            self.editor.workspace_id,
            self.shell.active_workspace,
        );
        let _ = self.shell.client.focus_window(self.shell.top_bar.window_id);
        self.push_notice(String::from("Session unlocked"));
        self.mark_shell_dirty();
    }

    pub(super) fn evaluate_boot_readiness(&mut self) {
        if self.shell.logged_in && !self.desktop_ready_published {
            crate::boot::appliance::publish_stage(crate::boot::appliance::BootStage::DesktopReady);
            self.desktop_ready_published = true;
        }

        let app_basket_ready = self.shell.logged_in
            && self.shell.top_bar.visible
            && self.shell.task_strip.visible
            && !self.shell.lock_screen.visible;

        if app_basket_ready && !self.app_basket_committed {
            crate::boot::appliance::publish_stage(
                crate::boot::appliance::BootStage::AppBasketReady,
            );
            crate::boot::appliance::mark_boot_success();
            self.app_basket_committed = true;
        }

        if app_basket_ready && self.suspend_resume_smoke_pending {
            self.suspend_resume_smoke_pending = false;
            crate::boot::appliance::clear_suspend_resume_smoke_request();
            crate::serial_println!("[SMOKE] suspend-resume arm");
            match crate::power::system_suspend() {
                Ok(()) => {
                    crate::serial_println!("[SMOKE] suspend-resume ok");
                    self.push_notice(String::from("Suspend/resume smoke completed"));
                    self.mark_shell_dirty();
                }
                Err(err) => {
                    crate::serial_println!("[SMOKE] suspend-resume fail: {:?}", err);
                    self.push_notice(String::from("Suspend/resume smoke failed"));
                    self.mark_shell_dirty();
                }
            }
        }

        if app_basket_ready {
            if let Some(bundle) = crate::boot::appliance::take_packaged_pe_smoke_bundle() {
                self.run_packaged_pe_smoke(bundle);
            }
        }
    }

    pub(super) fn run_packaged_pe_smoke(&mut self, bundle: Vec<u8>) {
        crate::serial_println!("[SMOKE] packaged-pe install arm");
        let inspection = match crate::security::package::inspect_signed_bundle(&bundle) {
            Ok(inspection) => inspection,
            Err(err) => {
                crate::serial_println!("[SMOKE] packaged-pe inspect fail: {}", err);
                self.push_notice(String::from("Packaged PE smoke inspect failed"));
                self.mark_shell_dirty();
                return;
            }
        };
        if inspection.compiled_manifest.runtime != echos_manifest::AppRuntime::Pe {
            crate::serial_println!(
                "[SMOKE] packaged-pe inspect fail: runtime={}",
                inspection.compiled_manifest.runtime.as_str()
            );
            self.push_notice(String::from("Packaged PE smoke runtime mismatch"));
            self.mark_shell_dirty();
            return;
        }
        let app_id = inspection.compiled_manifest.app_id.clone();
        match crate::security::package::install_bundle(&bundle) {
            Ok(_) => {
                crate::serial_println!("[SMOKE] packaged-pe install ok app_id={}", app_id);
            }
            Err(err) => {
                crate::serial_println!("[SMOKE] packaged-pe install fail: {}", err);
                self.push_notice(String::from("Packaged PE smoke install failed"));
                self.mark_shell_dirty();
                return;
            }
        }

        let Some(resolution) = self.launch_resolution(app_id.as_str()) else {
            crate::serial_println!("[SMOKE] packaged-pe launch fail: unresolved");
            self.push_notice(String::from("Packaged PE smoke launch unresolved"));
            self.mark_shell_dirty();
            return;
        };
        let intent = resolution.launch_intent(ExecutionContext::new(
            LaunchSource::ShellShortcut,
            self.shell.active_workspace,
            "packaged-pe-smoke",
        ));
        match self.dispatch_launch_session(intent.canonical_session(), resolution.path()) {
            Ok(()) => {
                crate::serial_println!("[SMOKE] packaged-pe launch ok app_id={}", app_id);
                self.push_notice(String::from("Packaged PE smoke launched"));
            }
            Err(err) => {
                crate::serial_println!("[SMOKE] packaged-pe launch fail: {}", err);
                self.push_notice(String::from("Packaged PE smoke launch failed"));
            }
        }
        self.mark_shell_dirty();
    }

    pub(super) fn toggle_power_state(&mut self) {
        let next = match self.shell.client.session_snapshot() {
            Ok(snapshot) if snapshot.power_state == SessionPowerState::Active => {
                SessionPowerState::Locked
            }
            _ => SessionPowerState::Active,
        };
        let _ = self.shell.client.set_power_state(next);
        if next == SessionPowerState::Locked {
            self.set_login_visibility(true);
            let _ = self
                .shell
                .client
                .focus_window(self.shell.lock_screen.window_id);
        } else {
            self.unlock_session();
        }
        self.push_notice(format!("Session {}", power_state_label(next)));
        self.invalidate_shell(InvalidationTarget::TopBar, InvalidationReason::StateChanged);
        self.invalidate_shell(
            InvalidationTarget::Launcher,
            InvalidationReason::StateChanged,
        );
    }

    pub(super) fn service_dialog_queue(&mut self) {
        if self.shell.pending_dialog.is_none() {
            let Ok(requests) = dialog_client_contract::list_pending_dialogs(1) else {
                return;
            };

            if let Some(request) = requests.into_iter().next() {
                self.close_context_menu();
                self.close_switcher();
                self.shell.dialog_input = dialog_default_path(&request);
                self.shell.pending_dialog = Some(request);
                animate_shell_surface(
                    &self.shell.client,
                    &mut self.shell.dialog,
                    true,
                    self.settings.animations,
                    0.16,
                );
                let _ = self.shell.client.focus_window(self.shell.dialog.window_id);
                self.invalidate_shell(InvalidationTarget::Dialog, InvalidationReason::StateChanged);
                self.push_notice(String::from("Dialog awaiting shell decision"));
            }
        }

        if self.shell.pending_dialog.is_none()
            && (self.shell.dialog.visible || self.shell.dialog.desired_visible)
        {
            animate_shell_surface(
                &self.shell.client,
                &mut self.shell.dialog,
                false,
                self.settings.animations,
                0.12,
            );
            self.invalidate_shell(InvalidationTarget::Dialog, InvalidationReason::StateChanged);
        }
    }

    pub(super) fn resolve_pending_dialog(&mut self, accept: bool) {
        let Some(request) = self.shell.pending_dialog.take() else {
            return;
        };

        let selection = if accept {
            let raw_path = if self.shell.dialog_input.is_empty() {
                dialog_default_path(&request)
            } else {
                self.shell.dialog_input.clone()
            };
            let path = match validate_dialog_selection(&request, &raw_path) {
                Ok(path) => path,
                Err(err) => {
                    self.shell.pending_dialog = Some(request);
                    self.push_notice(err);
                    self.invalidate_shell(
                        InvalidationTarget::Dialog,
                        InvalidationReason::StateChanged,
                    );
                    return;
                }
            };
            if matches!(
                request.kind,
                DialogKind::OpenFile | DialogKind::SaveFile | DialogKind::PickFolder
            ) {
                let _ = shell_client_contract::grant_file_access(request.app_id, &path, false);
            }
            DialogSelection::Accepted(path)
        } else {
            DialogSelection::Cancelled
        };
        let _ = dialog_client_contract::resolve_dialog(request.id, selection);
        self.shell.dialog_input.clear();
        animate_shell_surface(
            &self.shell.client,
            &mut self.shell.dialog,
            false,
            self.settings.animations,
            0.12,
        );
        self.invalidate_shell(InvalidationTarget::Dialog, InvalidationReason::StateChanged);
        self.push_notice(format!("Dialog resolved for app {}", request.app_id));
    }
}
