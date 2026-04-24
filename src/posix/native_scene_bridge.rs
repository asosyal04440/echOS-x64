use alloc::string::ToString;
use alloc::vec::Vec;

use super::super::gfx::image_assets::ArgbImage;
use super::super::gui::launch_pipeline::{
    DpiVirtualizationMode, ExternalDisplayContract, RuntimeBootstrap,
};
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
    let request = match super::read_user::<super::NativeWindowCreateRequest>(req_ptr) {
        Ok(value) => value,
        Err(err) => return err,
    };
    if let Err(err) =
        super::validate_user_range(out_ptr, core::mem::size_of::<super::NativeWindowHandle>())
    {
        return err;
    }
    let runtime = match current_native_runtime() {
        Ok(runtime) => runtime,
        Err(err) => return err,
    };
    let title = match super::decode_inline_text(&request.title, request.title_len) {
        Ok(title) => title,
        Err(err) => return err,
    };
    let geometry = scale_window_geometry(&runtime.session.process.external_display, &request);

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
            x: geometry.physical_x,
            y: geometry.physical_y,
            width: geometry.physical_width,
            height: geometry.physical_height,
        },
    );
    match response {
        Some(display_client_contract::DisplayResponse::WindowCreated {
            window_id,
            surface_id,
            ..
        }) => {
            let workspace_id = runtime.session.window.workspace_id;
            native_scene_contract::attach_window_session_with_display(
                runtime.identity.app_id,
                workspace_id,
                false,
                window_id,
                surface_id,
                runtime.session.process.external_display,
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
                content_width: geometry.logical_width,
                content_height: geometry.logical_height,
            };
            if let Err(err) = super::write_user(out_ptr, handle) {
                return err;
            }
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
    let request = match super::read_user::<super::NativeSceneSubmitRequest>(req_ptr) {
        Ok(value) => value,
        Err(err) => return err,
    };
    let op_count = request.op_count as usize;
    if op_count > super::MAX_SCENE_OPS || request.ops_ptr == 0 {
        return super::errno(super::EINVAL);
    }
    let op_size = core::mem::size_of::<super::NativeSceneOp>();
    let ops_bytes = op_count.saturating_mul(op_size);
    if let Err(err) = super::validate_user_range(request.ops_ptr as usize, ops_bytes) {
        return err;
    }
    let runtime = match current_native_runtime() {
        Ok(runtime) => runtime,
        Err(err) => return err,
    };
    let mut raw_ops = Vec::with_capacity(op_count);
    for index in 0..op_count {
        let op_ptr = (request.ops_ptr as usize).saturating_add(index.saturating_mul(op_size));
        let op = match super::read_user::<super::NativeSceneOp>(op_ptr) {
            Ok(value) => value,
            Err(err) => return err,
        };
        raw_ops.push(op);
    }
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
    apply_display_virtualization(&mut scene, runtime.session.process.external_display);
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
    let request = match super::read_user::<super::NativeNotificationRequest>(req_ptr) {
        Ok(value) => value,
        Err(err) => return err,
    };
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
    let request = match super::read_user::<super::NativeClipboardSetTextRequest>(req_ptr) {
        Ok(value) => value,
        Err(err) => return err,
    };
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
    if let Err(err) = super::validate_user_range(
        resp_ptr,
        core::mem::size_of::<super::NativeClipboardGetTextResponse>(),
    ) {
        return err;
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
            if let Err(err) = super::write_user(resp_ptr, response) {
                return err;
            }
            0
        }
        Some(clipboard_client_contract::ClipboardResponse::Current(ClipboardPayload::Empty)) => {
            if let Err(err) = super::write_user(
                resp_ptr,
                super::NativeClipboardGetTextResponse {
                    text_len: 0,
                    text: [0u8; super::MAX_INLINE_TEXT],
                },
            ) {
                return err;
            }
            0
        }
        Some(clipboard_client_contract::ClipboardResponse::Error(_)) => super::errno(super::EACCES),
        _ => super::errno(super::EIO),
    }
}

pub(super) fn sys_native_event_poll(out_ptr: usize, max_events: usize) -> usize {
    let max_events = max_events.clamp(1, super::MAX_POLLED_EVENTS);
    let out_bytes = max_events.saturating_mul(core::mem::size_of::<super::NativeInputEvent>());
    if let Err(err) = super::validate_user_range(out_ptr, out_bytes) {
        return err;
    }
    let runtime = match current_native_runtime() {
        Ok(runtime) => runtime,
        Err(err) => return err,
    };
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
                .map(|event| {
                    map_native_input_event(event, runtime.session.process.external_display)
                })
                .collect();
            let event_size = core::mem::size_of::<super::NativeInputEvent>();
            for (index, event) in translated.iter().enumerate() {
                let dst_ptr = out_ptr.saturating_add(index.saturating_mul(event_size));
                if let Err(err) = super::write_user(dst_ptr, *event) {
                    return err;
                }
            }
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

#[derive(Clone, Copy)]
struct ScaledWindowGeometry {
    physical_x: i32,
    physical_y: i32,
    physical_width: u32,
    physical_height: u32,
    logical_width: u32,
    logical_height: u32,
}

fn scale_window_geometry(
    contract: &ExternalDisplayContract,
    request: &super::NativeWindowCreateRequest,
) -> ScaledWindowGeometry {
    let logical_width = request.width.max(1);
    let logical_height = request.height.max(1);
    if !uses_bitmap_virtualization(*contract) {
        return ScaledWindowGeometry {
            physical_x: request.x,
            physical_y: request.y,
            physical_width: logical_width,
            physical_height: logical_height,
            logical_width,
            logical_height,
        };
    }

    ScaledWindowGeometry {
        physical_x: scale_i32_round(request.x, contract.ui_scale_100x),
        physical_y: scale_i32_round(request.y, contract.ui_scale_100x),
        physical_width: scale_u32_round(logical_width, contract.ui_scale_100x).max(1),
        physical_height: scale_u32_round(logical_height, contract.ui_scale_100x).max(1),
        logical_width,
        logical_height,
    }
}

fn apply_display_virtualization(scene: &mut SceneUpdate, contract: ExternalDisplayContract) {
    if !uses_bitmap_virtualization(contract) {
        return;
    }

    for rect in scene.damage_hint.iter_mut() {
        *rect = scale_rect(*rect, contract.ui_scale_100x);
    }

    for object in scene.render_objects.iter_mut() {
        let object_scale = match object.kind {
            RenderObjectKind::TextRun { .. } | RenderObjectKind::GlyphRun { .. } => {
                contract.text_scale_100x
            }
            _ => contract.ui_scale_100x,
        };
        object.bounds = scale_rect(object.bounds, object_scale);
        if let Some(clip) = object.clip {
            object.clip = Some(scale_rect(clip, object_scale));
        }
        match &mut object.kind {
            RenderObjectKind::SolidRect { corner_radius, .. } => {
                *corner_radius = scale_u16_round(*corner_radius, contract.ui_scale_100x);
            }
            RenderObjectKind::Raster {
                width,
                height,
                pixels,
            } => {
                let target_width = scale_u32_round(*width, contract.ui_scale_100x).max(1);
                let target_height = scale_u32_round(*height, contract.ui_scale_100x).max(1);
                if target_width == *width && target_height == *height {
                    continue;
                }
                if let Ok(resized) = (ArgbImage {
                    width: *width,
                    height: *height,
                    pixels: pixels.clone(),
                })
                .resize_exact(target_width, target_height)
                {
                    *width = resized.width;
                    *height = resized.height;
                    *pixels = resized.pixels;
                }
            }
            RenderObjectKind::TextRun { max_width, .. } => {
                *max_width = scale_u32_round(*max_width, contract.text_scale_100x).max(1);
            }
            RenderObjectKind::GlyphRun { width, height, .. } => {
                *width = scale_u32_round(*width, contract.text_scale_100x).max(1);
                *height = scale_u32_round(*height, contract.text_scale_100x).max(1);
            }
        }
    }
}

fn map_native_input_event(
    event: &WindowInputEvent,
    contract: ExternalDisplayContract,
) -> super::NativeInputEvent {
    let uses_virtualization = uses_bitmap_virtualization(contract);
    match &event.event {
        InputEvent::Key {
            scan_code,
            modifiers,
            state,
            ..
        } => super::NativeInputEvent {
            kind: super::NativeEventKind::Key as u32,
            window_id: event.window_id,
            x: event.local_position.map_or(0, |point| {
                if uses_virtualization {
                    unscale_i32_round(point.x, contract.ui_scale_100x)
                } else {
                    point.x
                }
            }),
            y: event.local_position.map_or(0, |point| {
                if uses_virtualization {
                    unscale_i32_round(point.y, contract.ui_scale_100x)
                } else {
                    point.y
                }
            }),
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
            x: if uses_virtualization {
                unscale_i32_round(position.x, contract.ui_scale_100x)
            } else {
                position.x
            },
            y: if uses_virtualization {
                unscale_i32_round(position.y, contract.ui_scale_100x)
            } else {
                position.y
            },
            delta_x: if uses_virtualization {
                unscale_i32_round(delta.x, contract.ui_scale_100x)
            } else {
                delta.x
            },
            delta_y: if uses_virtualization {
                unscale_i32_round(delta.y, contract.ui_scale_100x)
            } else {
                delta.y
            },
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
            x: if uses_virtualization {
                unscale_i32_round(position.x, contract.ui_scale_100x)
            } else {
                position.x
            },
            y: if uses_virtualization {
                unscale_i32_round(position.y, contract.ui_scale_100x)
            } else {
                position.y
            },
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
            x: if uses_virtualization {
                unscale_i32_round(position.x, contract.ui_scale_100x)
            } else {
                position.x
            },
            y: if uses_virtualization {
                unscale_i32_round(position.y, contract.ui_scale_100x)
            } else {
                position.y
            },
            delta_x: if uses_virtualization {
                unscale_i32_round(delta.x, contract.ui_scale_100x)
            } else {
                delta.x
            },
            delta_y: if uses_virtualization {
                unscale_i32_round(delta.y, contract.ui_scale_100x)
            } else {
                delta.y
            },
            key_code: 0,
            modifiers: 0,
            state: 0,
            button: 0,
            reserved: 0,
        },
    }
}

fn uses_bitmap_virtualization(contract: ExternalDisplayContract) -> bool {
    matches!(
        contract.dpi_virtualization,
        DpiVirtualizationMode::BitmapScale
    ) && contract.ui_scale_100x > 100
}

fn scale_rect(rect: Rect, scale_100x: u16) -> Rect {
    Rect::new(
        scale_i32_round(rect.x, scale_100x),
        scale_i32_round(rect.y, scale_100x),
        scale_u32_round(rect.width, scale_100x),
        scale_u32_round(rect.height, scale_100x),
    )
}

fn scale_i32_round(value: i32, scale_100x: u16) -> i32 {
    let scale = scale_100x.max(1) as i64;
    let value = value as i64;
    let adjusted = if value >= 0 {
        value.saturating_mul(scale).saturating_add(50)
    } else {
        value.saturating_mul(scale).saturating_sub(50)
    };
    adjusted
        .checked_div(100)
        .unwrap_or(0)
        .clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

fn scale_u32_round(value: u32, scale_100x: u16) -> u32 {
    (value as u64)
        .saturating_mul(scale_100x.max(1) as u64)
        .saturating_add(50)
        .checked_div(100)
        .unwrap_or(0)
        .min(u32::MAX as u64) as u32
}

fn scale_u16_round(value: u16, scale_100x: u16) -> u16 {
    scale_u32_round(value as u32, scale_100x).min(u16::MAX as u32) as u16
}

fn unscale_i32_round(value: i32, scale_100x: u16) -> i32 {
    let scale = scale_100x.max(1) as i64;
    let value = value as i64;
    let adjusted = if value >= 0 {
        value.saturating_mul(100).saturating_add(scale / 2)
    } else {
        value.saturating_mul(100).saturating_sub(scale / 2)
    };
    adjusted
        .checked_div(scale)
        .unwrap_or(0)
        .clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

#[cfg(test)]
mod tests {
    use super::{
        apply_display_virtualization, map_native_input_event, scale_window_geometry,
        uses_bitmap_virtualization,
    };
    use crate::gui::launch_pipeline::{DpiVirtualizationMode, ExternalDisplayContract};
    use crate::gui::protocol::{
        DamageLane, InputEvent, KeyState, Point, Rect, RenderObject, RenderObjectKind, SceneUpdate,
        TextRunStyle, WindowInputEvent,
    };
    use echos_sdk_sys::NativeWindowCreateRequest;

    #[test]
    fn bitmap_virtualization_scales_window_geometry() {
        let contract = ExternalDisplayContract {
            output_id: 2,
            ui_scale_100x: 150,
            text_scale_100x: 175,
            cursor_scale_100x: 125,
            dpi_virtualization: DpiVirtualizationMode::BitmapScale,
        };
        assert!(uses_bitmap_virtualization(contract));
        let request = NativeWindowCreateRequest {
            title_len: 0,
            title: [0; echos_sdk_sys::MAX_INLINE_TEXT],
            x: 20,
            y: 10,
            width: 640,
            height: 360,
        };
        let geometry = scale_window_geometry(&contract, &request);
        assert_eq!(geometry.physical_x, 30);
        assert_eq!(geometry.physical_y, 15);
        assert_eq!(geometry.physical_width, 960);
        assert_eq!(geometry.physical_height, 540);
        assert_eq!(geometry.logical_width, 640);
        assert_eq!(geometry.logical_height, 360);
    }

    #[test]
    fn bitmap_virtualization_scales_scene_payloads() {
        let mut scene = SceneUpdate {
            root_id: 7,
            revision: 3,
            render_objects: alloc::vec![
                RenderObject {
                    object_id: 1,
                    bounds: Rect::new(4, 6, 20, 12),
                    clip: Some(Rect::new(0, 0, 10, 10)),
                    z_index: 1,
                    opacity: 255,
                    lane: DamageLane::Window,
                    kind: RenderObjectKind::SolidRect {
                        color: 0xFF223344,
                        corner_radius: 6,
                    },
                },
                RenderObject {
                    object_id: 2,
                    bounds: Rect::new(10, 20, 8, 4),
                    clip: None,
                    z_index: 2,
                    opacity: 255,
                    lane: DamageLane::Text,
                    kind: RenderObjectKind::TextRun {
                        blob_id: 11,
                        text: alloc::string::String::from("echOS"),
                        color: 0xFFFFFFFF,
                        style: TextRunStyle::Ui,
                        max_width: 80,
                    },
                },
                RenderObject {
                    object_id: 3,
                    bounds: Rect::new(2, 2, 2, 2),
                    clip: None,
                    z_index: 3,
                    opacity: 255,
                    lane: DamageLane::Window,
                    kind: RenderObjectKind::Raster {
                        width: 2,
                        height: 2,
                        pixels: alloc::vec![0xFF000000, 0xFFFFFFFF, 0xFF00FF00, 0xFFFF0000,],
                    },
                },
            ],
            damage_hint: alloc::vec![Rect::new(0, 0, 8, 8)],
            semantic_root: None,
        };
        apply_display_virtualization(
            &mut scene,
            ExternalDisplayContract {
                output_id: 1,
                ui_scale_100x: 150,
                text_scale_100x: 175,
                cursor_scale_100x: 125,
                dpi_virtualization: DpiVirtualizationMode::BitmapScale,
            },
        );
        assert_eq!(scene.damage_hint[0], Rect::new(0, 0, 12, 12));
        assert_eq!(scene.render_objects[0].bounds, Rect::new(6, 9, 30, 18));
        assert!(matches!(
            &scene.render_objects[0].kind,
            RenderObjectKind::SolidRect { corner_radius, .. } if *corner_radius == 9
        ));
        assert_eq!(scene.render_objects[1].bounds, Rect::new(18, 35, 14, 7));
        assert!(matches!(
            &scene.render_objects[1].kind,
            RenderObjectKind::TextRun { max_width, .. } if *max_width == 140
        ));
        assert!(matches!(
            &scene.render_objects[2].kind,
            RenderObjectKind::Raster { width, height, pixels } if (*width, *height) == (3, 3) && pixels.len() == 9
        ));
    }

    #[test]
    fn bitmap_virtualization_unscales_runtime_input() {
        let event = WindowInputEvent {
            app_id: 9,
            window_id: 41,
            local_position: Some(Point::new(150, 90)),
            global_position: Some(Point::new(300, 180)),
            captured: false,
            event: InputEvent::PointerMove {
                position: Point::new(150, 90),
                delta: Point::new(30, -15),
            },
        };
        let native = map_native_input_event(
            &event,
            ExternalDisplayContract {
                output_id: 0,
                ui_scale_100x: 150,
                text_scale_100x: 100,
                cursor_scale_100x: 100,
                dpi_virtualization: DpiVirtualizationMode::BitmapScale,
            },
        );
        assert_eq!(native.x, 100);
        assert_eq!(native.y, 60);
        assert_eq!(native.delta_x, 20);
        assert_eq!(native.delta_y, -10);
    }

    #[test]
    fn native_mode_keeps_input_coordinates() {
        let event = WindowInputEvent {
            app_id: 9,
            window_id: 41,
            local_position: Some(Point::new(80, 60)),
            global_position: None,
            captured: false,
            event: InputEvent::Key {
                scan_code: 30,
                unicode: None,
                modifiers: 0,
                state: KeyState::Pressed,
            },
        };
        let native = map_native_input_event(&event, ExternalDisplayContract::default());
        assert_eq!(native.x, 80);
        assert_eq!(native.y, 60);
        assert_eq!(native.key_code, 30);
    }

    #[test]
    fn native_bridge_syscalls_fail_closed_on_null_user_pointers() {
        let efault = super::super::errno(super::super::EFAULT);
        assert_eq!(super::sys_native_window_create(0, 0), efault);
        assert_eq!(super::sys_native_scene_commit(0), efault);
        assert_eq!(super::sys_native_notification_post(0), efault);
        assert_eq!(super::sys_native_clipboard_set_text(0), efault);
        assert_eq!(super::sys_native_clipboard_get_text(0), efault);
        assert_eq!(super::sys_native_event_poll(0, 1), efault);
    }
}
