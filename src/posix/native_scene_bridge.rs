use alloc::string::ToString;
use alloc::vec::Vec;

use super::super::gui::launch_pipeline::RuntimeBootstrap;
use super::super::gui::protocol::{
    ClipboardPayload, DamageLane, InputEvent, KeyState, NotificationLevel, NotificationRequest,
    PointerButton, Rect, RenderObject, RenderObjectKind, SceneUpdate, TextRunStyle,
    WindowInputEvent,
};
use super::super::runtime_layer::native_scene_contract::RuntimeHandle;
use super::super::runtime_layer::{
    capability_contract, clipboard_client_contract, display_client_contract, input_client_contract,
    native_scene_contract, notification_client_contract, shell_client_contract,
};
use super::super::task;

pub(super) fn sys_native_window_create(req_ptr: usize, out_ptr: usize) -> usize {
    if req_ptr == 0 || out_ptr == 0 {
        return super::errno(super::EFAULT);
    }
    let runtime = match current_native_runtime() {
        Ok(runtime) => runtime,
        Err(err) => return err,
    };
    let request = super::with_user_access(|| unsafe {
        *(req_ptr as *const super::NativeWindowCreateRequest)
    });
    let title = match super::decode_inline_text(&request.title, request.title_len) {
        Ok(title) => title,
        Err(err) => return err,
    };

    let _ = input_client_contract::request_input_sync(
        runtime.identity.app_id,
        input_client_contract::InputCommand::RegisterApp {
            app_id: runtime.identity.app_id,
        },
    );
    let _ = shell_client_contract::request_shell_sync(
        runtime.identity.app_id,
        shell_client_contract::ShellCommand::RegisterApp {
            app_id: runtime.identity.app_id,
            name: runtime.identity.title.to_string(),
        },
    );
    let _ = shell_client_contract::request_shell_sync(
        runtime.identity.app_id,
        shell_client_contract::ShellCommand::MarkAppLaunch {
            app_id: runtime.identity.app_id,
            status_line: alloc::string::String::from("Native SDK runtime active"),
        },
    );

    let response = display_client_contract::request_display_sync(
        runtime.identity.app_id,
        display_client_contract::DisplayCommand::CreateWindow {
            app_id: runtime.identity.app_id,
            title,
            x: request.x,
            y: request.y,
            width: request.width,
            height: request.height,
        },
    );
    match response {
        Some(display_client_contract::DisplayResponse::WindowCreated {
            window_id,
            surface_id,
            content_rect,
        }) => {
            let workspace_id = runtime.session.window.workspace_id;
            native_scene_contract::attach_window_session(
                runtime.identity.app_id,
                workspace_id,
                false,
                window_id,
                surface_id,
            );
            let _ = shell_client_contract::request_shell_sync(
                runtime.identity.app_id,
                shell_client_contract::ShellCommand::UpdateAppWindow {
                    app_id: runtime.identity.app_id,
                    window_id: Some(window_id),
                    visible: true,
                    focused: false,
                    workspace_id,
                },
            );
            let handle = super::NativeWindowHandle {
                window_id,
                surface_id,
                content_width: content_rect.width,
                content_height: content_rect.height,
            };
            super::with_user_access(|| unsafe {
                *(out_ptr as *mut super::NativeWindowHandle) = handle;
            });
            0
        }
        Some(display_client_contract::DisplayResponse::Error(_)) => super::errno(super::EINVAL),
        _ => super::errno(super::EIO),
    }
}

pub(super) fn sys_native_window_destroy(window_id: usize) -> usize {
    let runtime = match current_native_runtime() {
        Ok(runtime) => runtime,
        Err(err) => return err,
    };
    let response = display_client_contract::request_display_sync(
        runtime.identity.app_id,
        display_client_contract::DisplayCommand::DestroyWindow {
            window_id: window_id as u64,
        },
    );
    match response {
        Some(display_client_contract::DisplayResponse::Ack) => {
            native_scene_contract::forget_window_session(window_id as u64);
            let _ = shell_client_contract::request_shell_sync(
                runtime.identity.app_id,
                shell_client_contract::ShellCommand::UpdateAppWindow {
                    app_id: runtime.identity.app_id,
                    window_id: None,
                    visible: false,
                    focused: false,
                    workspace_id: runtime.session.window.workspace_id,
                },
            );
            0
        }
        Some(display_client_contract::DisplayResponse::Error(_)) => super::errno(super::EINVAL),
        _ => super::errno(super::EIO),
    }
}

pub(super) fn sys_native_scene_commit(req_ptr: usize) -> usize {
    if req_ptr == 0 {
        return super::errno(super::EFAULT);
    }
    let runtime = match current_native_runtime() {
        Ok(runtime) => runtime,
        Err(err) => return err,
    };
    let request =
        super::with_user_access(|| unsafe { *(req_ptr as *const super::NativeSceneSubmitRequest) });
    if request.op_count as usize > super::MAX_SCENE_OPS || request.ops_ptr == 0 {
        return super::errno(super::EINVAL);
    }
    let raw_ops = super::with_user_access(|| unsafe {
        core::slice::from_raw_parts(
            request.ops_ptr as *const super::NativeSceneOp,
            request.op_count as usize,
        )
    });
    let mut render_objects = Vec::with_capacity(raw_ops.len());
    for (index, raw) in raw_ops.iter().enumerate() {
        let bounds = Rect::new(raw.x, raw.y, raw.width, raw.height);
        let kind = match raw.kind {
            value if value == super::NativeSceneOpKind::SolidRect as u32 => {
                RenderObjectKind::SolidRect {
                    color: raw.color,
                    corner_radius: raw.corner_radius,
                }
            }
            value if value == super::NativeSceneOpKind::Text as u32 => {
                let text = match super::decode_inline_text(&raw.text, raw.text_len) {
                    Ok(text) => text,
                    Err(err) => return err,
                };
                RenderObjectKind::TextRun {
                    blob_id: index as u64 + 1,
                    text,
                    color: raw.color,
                    style: if raw.style_flags & 1 != 0 {
                        TextRunStyle::Mono
                    } else {
                        TextRunStyle::Ui
                    },
                    max_width: raw.width,
                }
            }
            _ => return super::errno(super::EINVAL),
        };
        render_objects.push(RenderObject {
            object_id: index as u64 + 1,
            bounds,
            clip: None,
            z_index: raw.z_index,
            opacity: raw.opacity,
            lane: if raw.kind == super::NativeSceneOpKind::Text as u32 {
                DamageLane::Text
            } else {
                DamageLane::Window
            },
            kind,
        });
    }
    let mut scene = SceneUpdate {
        root_id: request.window_id,
        revision: request.revision,
        render_objects,
        damage_hint: Vec::new(),
        semantic_root: None,
    };
    scene.canonicalize();
    match display_client_contract::request_display_sync(
        runtime.identity.app_id,
        display_client_contract::DisplayCommand::CommitScene {
            window_id: request.window_id,
            scene,
        },
    ) {
        Some(display_client_contract::DisplayResponse::Ack) => 0,
        Some(display_client_contract::DisplayResponse::Error(_)) => super::errno(super::EINVAL),
        _ => super::errno(super::EIO),
    }
}

pub(super) fn sys_native_notification_post(req_ptr: usize) -> usize {
    if req_ptr == 0 {
        return super::errno(super::EFAULT);
    }
    let runtime = match current_native_runtime() {
        Ok(runtime) => runtime,
        Err(err) => return err,
    };
    if !capability_contract::task_allows_native_capability(
        task::scheduler::current_task_id() as u64,
        echos_manifest::NativeCapability::NotificationsPost,
    ) {
        return super::errno(super::EACCES);
    }
    let request = super::with_user_access(|| unsafe {
        *(req_ptr as *const super::NativeNotificationRequest)
    });
    let title = match super::decode_inline_text(&request.title, request.title_len) {
        Ok(text) => text,
        Err(err) => return err,
    };
    let message = match super::decode_inline_text(&request.message, request.message_len) {
        Ok(text) => text,
        Err(err) => return err,
    };
    let level = match request.level {
        0 => NotificationLevel::Info,
        1 => NotificationLevel::Success,
        2 => NotificationLevel::Warning,
        3 => NotificationLevel::Error,
        _ => return super::errno(super::EINVAL),
    };
    match notification_client_contract::request_notification_sync(
        runtime.identity.app_id,
        notification_client_contract::NotificationCommand::Push(NotificationRequest {
            app_id: runtime.identity.app_id,
            title,
            message,
            level,
            action_label: None,
        }),
    ) {
        Some(notification_client_contract::NotificationResponse::NotificationId(_))
        | Some(notification_client_contract::NotificationResponse::Ack) => 0,
        Some(notification_client_contract::NotificationResponse::Error(_)) => {
            super::errno(super::EACCES)
        }
        _ => super::errno(super::EIO),
    }
}

pub(super) fn sys_native_clipboard_set_text(req_ptr: usize) -> usize {
    if req_ptr == 0 {
        return super::errno(super::EFAULT);
    }
    let runtime = match current_native_runtime() {
        Ok(runtime) => runtime,
        Err(err) => return err,
    };
    if !capability_contract::task_allows_native_capability(
        task::scheduler::current_task_id() as u64,
        echos_manifest::NativeCapability::ClipboardWrite,
    ) {
        return super::errno(super::EACCES);
    }
    let request = super::with_user_access(|| unsafe {
        *(req_ptr as *const super::NativeClipboardSetTextRequest)
    });
    let text = match super::decode_inline_text(&request.text, request.text_len) {
        Ok(text) => text,
        Err(err) => return err,
    };
    match clipboard_client_contract::request_clipboard_sync(
        runtime.identity.app_id,
        clipboard_client_contract::ClipboardCommand::Set {
            app_id: runtime.identity.app_id,
            payload: ClipboardPayload::Text(text),
        },
    ) {
        Some(clipboard_client_contract::ClipboardResponse::Ack) => 0,
        Some(clipboard_client_contract::ClipboardResponse::Error(_)) => super::errno(super::EACCES),
        _ => super::errno(super::EIO),
    }
}

pub(super) fn sys_native_clipboard_get_text(resp_ptr: usize) -> usize {
    if resp_ptr == 0 {
        return super::errno(super::EFAULT);
    }
    let runtime = match current_native_runtime() {
        Ok(runtime) => runtime,
        Err(err) => return err,
    };
    if !capability_contract::task_allows_native_capability(
        task::scheduler::current_task_id() as u64,
        echos_manifest::NativeCapability::ClipboardRead,
    ) {
        return super::errno(super::EACCES);
    }
    match clipboard_client_contract::request_clipboard_sync(
        runtime.identity.app_id,
        clipboard_client_contract::ClipboardCommand::GetCurrent {
            app_id: runtime.identity.app_id,
        },
    ) {
        Some(clipboard_client_contract::ClipboardResponse::Current(ClipboardPayload::Text(
            text,
        ))) => {
            if text.len() > super::MAX_INLINE_TEXT {
                return super::errno(super::EFBIG);
            }
            let mut response = super::NativeClipboardGetTextResponse {
                text_len: text.len() as u16,
                text: [0u8; super::MAX_INLINE_TEXT],
            };
            response.text[..text.len()].copy_from_slice(text.as_bytes());
            super::with_user_access(|| unsafe {
                *(resp_ptr as *mut super::NativeClipboardGetTextResponse) = response;
            });
            0
        }
        Some(clipboard_client_contract::ClipboardResponse::Current(ClipboardPayload::Empty)) => {
            super::with_user_access(|| unsafe {
                *(resp_ptr as *mut super::NativeClipboardGetTextResponse) =
                    super::NativeClipboardGetTextResponse {
                        text_len: 0,
                        text: [0u8; super::MAX_INLINE_TEXT],
                    };
            });
            0
        }
        Some(clipboard_client_contract::ClipboardResponse::Error(_)) => super::errno(super::EACCES),
        _ => super::errno(super::EIO),
    }
}

pub(super) fn sys_native_event_poll(out_ptr: usize, max_events: usize) -> usize {
    if out_ptr == 0 {
        return super::errno(super::EFAULT);
    }
    let runtime = match current_native_runtime() {
        Ok(runtime) => runtime,
        Err(err) => return err,
    };
    let max_events = max_events.clamp(1, super::MAX_POLLED_EVENTS);
    match input_client_contract::request_input_sync(
        runtime.identity.app_id,
        input_client_contract::InputCommand::PollEvents {
            app_id: runtime.identity.app_id,
            max_events,
        },
    ) {
        Some(input_client_contract::InputResponse::Events { events, .. }) => {
            let translated: Vec<super::NativeInputEvent> = events
                .iter()
                .take(max_events)
                .map(map_native_input_event)
                .collect();
            super::with_user_access(|| unsafe {
                let out = core::slice::from_raw_parts_mut(
                    out_ptr as *mut super::NativeInputEvent,
                    translated.len(),
                );
                out.copy_from_slice(&translated);
            });
            translated.len()
        }
        Some(input_client_contract::InputResponse::Error(_)) => super::errno(super::EIO),
        _ => 0,
    }
}

fn current_native_runtime() -> Result<RuntimeHandle, usize> {
    let task_id = task::scheduler::current_task_id() as u64;
    let Some(runtime) = native_scene_contract::runtime_handle_for_task(task_id) else {
        return Err(super::errno(super::EACCES));
    };
    match runtime.session.process.bootstrap {
        RuntimeBootstrap::NativeWindowed | RuntimeBootstrap::NativeHeadless => Ok(runtime),
        _ => Err(super::errno(super::EACCES)),
    }
}

fn map_native_input_event(event: &WindowInputEvent) -> super::NativeInputEvent {
    match &event.event {
        InputEvent::Key {
            scan_code,
            modifiers,
            state,
            ..
        } => super::NativeInputEvent {
            kind: super::NativeEventKind::Key as u32,
            window_id: event.window_id,
            x: event
                .local_position
                .map(|point| point.x)
                .unwrap_or_default(),
            y: event
                .local_position
                .map(|point| point.y)
                .unwrap_or_default(),
            delta_x: 0,
            delta_y: 0,
            key_code: *scan_code as u32,
            modifiers: *modifiers,
            state: matches!(state, KeyState::Pressed) as u8,
            button: 0,
            reserved: 0,
        },
        InputEvent::PointerMove { position, delta } => super::NativeInputEvent {
            kind: super::NativeEventKind::PointerMove as u32,
            window_id: event.window_id,
            x: position.x,
            y: position.y,
            delta_x: delta.x,
            delta_y: delta.y,
            key_code: 0,
            modifiers: 0,
            state: 0,
            button: 0,
            reserved: 0,
        },
        InputEvent::PointerButton {
            button,
            state,
            position,
        } => super::NativeInputEvent {
            kind: super::NativeEventKind::PointerButton as u32,
            window_id: event.window_id,
            x: position.x,
            y: position.y,
            delta_x: 0,
            delta_y: 0,
            key_code: 0,
            modifiers: 0,
            state: matches!(state, KeyState::Pressed) as u8,
            button: match button {
                PointerButton::Left => 1,
                PointerButton::Right => 2,
                PointerButton::Middle => 3,
                PointerButton::Other(value) => *value,
            },
            reserved: 0,
        },
        InputEvent::Scroll { position, delta } => super::NativeInputEvent {
            kind: super::NativeEventKind::PointerMove as u32,
            window_id: event.window_id,
            x: position.x,
            y: position.y,
            delta_x: delta.x,
            delta_y: delta.y,
            key_code: 0,
            modifiers: 0,
            state: 0,
            button: 0,
            reserved: 0,
        },
    }
}
