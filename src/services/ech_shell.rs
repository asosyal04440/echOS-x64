//! Session shell registry, permissions, and recovery metadata service.

use crate::services::display_atomic::MailboxRing;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

const SHELL_COMMAND_QUEUE_CAPACITY: usize = 256;
const SHELL_RESPONSE_QUEUE_CAPACITY: usize = 256;
const ACCESSIBILITY_EVENT_CAPACITY: usize = 64;
const CAPTION_EVENT_CAPACITY: usize = 24;
const SPEECH_EVENT_CAPACITY: usize = 16;
const SPEECH_RETRY_BACKOFF_NS: u64 = 250_000_000;

use crate::gui::protocol::{
    AccessibilityEvent, AccessibilityEventKind, AccessibilityFocusState, AccessibilityNode,
    AccessibilityProfile, AccessibilityRole, AppHealth, AppId, CaptionEvent, DesktopPermission,
    DisplayProfile, FileGrant, MagnifierMode, MotionProfile, PermissionEntry, PermissionState,
    RestoreDisposition, SessionPowerState, SessionSnapshot, ShellAppEntry, ShellDensityProfile,
    SpeechEvent, SpeechState, StageSet, StageSetPolicy, WindowId, WindowRule, WorkspaceId,
    WorkspaceLayout, WorkspaceRule,
};

#[derive(Default)]
struct SpeechLane {
    active: Option<SpeechEvent>,
    pending: Vec<SpeechEvent>,
    dropped_count: u32,
    coalesced_count: u32,
    active_deadline_ns: u64,
}

impl SpeechLane {
    fn state(&self, max_items: usize) -> SpeechState {
        SpeechState {
            active: self.active.clone(),
            pending: self
                .pending
                .iter()
                .take(max_items.max(1))
                .cloned()
                .collect(),
            dropped_count: self.dropped_count,
            coalesced_count: self.coalesced_count,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpeechOutputHealth {
    Ready,
    Recovering,
    FailedClosed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpeechOutputErrorKind {
    AudioInvalidChannel,
    AudioEmptyPayload,
    AudioQueueSaturated,
    AudioUnsupportedFormat,
    AudioServiceUnavailable,
    UnsupportedLocale,
    UnknownVoice,
    VoiceLocaleMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpeechOutputError {
    pub kind: SpeechOutputErrorKind,
    pub detail: String,
    pub retryable: bool,
}

impl SpeechOutputError {
    fn from_audio_error(error: crate::services::AudioError) -> Self {
        let kind = match error.kind {
            crate::services::AudioErrorKind::InvalidChannel => {
                SpeechOutputErrorKind::AudioInvalidChannel
            }
            crate::services::AudioErrorKind::EmptyPayload => {
                SpeechOutputErrorKind::AudioEmptyPayload
            }
            crate::services::AudioErrorKind::QueueSaturated => {
                SpeechOutputErrorKind::AudioQueueSaturated
            }
            crate::services::AudioErrorKind::UnsupportedFormat => {
                SpeechOutputErrorKind::AudioUnsupportedFormat
            }
            crate::services::AudioErrorKind::ServiceUnavailable => {
                SpeechOutputErrorKind::AudioServiceUnavailable
            }
        };
        Self {
            kind,
            detail: error.detail,
            retryable: error.retryable,
        }
    }

    fn from_voice_error(error: crate::audio::tts::VoiceSelectionError) -> Self {
        let kind = match error.kind {
            crate::audio::tts::VoiceSelectionErrorKind::UnknownVoice => {
                SpeechOutputErrorKind::UnknownVoice
            }
            crate::audio::tts::VoiceSelectionErrorKind::UnsupportedLocale => {
                SpeechOutputErrorKind::UnsupportedLocale
            }
            crate::audio::tts::VoiceSelectionErrorKind::VoiceLocaleMismatch => {
                SpeechOutputErrorKind::VoiceLocaleMismatch
            }
        };
        Self {
            kind,
            detail: error.detail,
            retryable: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpeechOutputStatus {
    pub channel_id: Option<u32>,
    pub health: SpeechOutputHealth,
    pub failure_count: u32,
    pub retry_after_ns: u64,
    pub locale: String,
    pub preferred_voice_id: Option<String>,
    pub resolved_voice_id: Option<String>,
    pub last_error: Option<SpeechOutputError>,
}

impl Default for SpeechOutputStatus {
    fn default() -> Self {
        Self {
            channel_id: None,
            health: SpeechOutputHealth::Ready,
            failure_count: 0,
            retry_after_ns: 0,
            locale: String::from("en-US"),
            preferred_voice_id: None,
            resolved_voice_id: None,
            last_error: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct SpeechOutputState {
    status: SpeechOutputStatus,
}

impl SpeechOutputState {
    fn snapshot(&self) -> SpeechOutputStatus {
        self.status.clone()
    }
}

#[derive(Clone, Debug)]
pub enum ShellCommand {
    RegisterApp {
        app_id: AppId,
        name: String,
    },
    UnregisterApp {
        app_id: AppId,
    },
    UpdateAppWindow {
        app_id: AppId,
        window_id: Option<WindowId>,
        visible: bool,
        focused: bool,
        workspace_id: WorkspaceId,
    },
    MarkAppLaunch {
        app_id: AppId,
        status_line: String,
    },
    MarkAppExit {
        app_id: AppId,
        clean: bool,
        status_line: String,
    },
    RecordAppFault {
        app_id: AppId,
        detail: String,
    },
    ClearAppAttention {
        app_id: AppId,
        status_line: Option<String>,
    },
    SetAutoRestore {
        app_id: AppId,
        enabled: bool,
    },
    SetPermission {
        app_id: AppId,
        permission: DesktopPermission,
        state: PermissionState,
    },
    GetPermission {
        app_id: AppId,
        permission: DesktopPermission,
    },
    ListPermissions {
        app_id: AppId,
    },
    GrantFileAccess {
        app_id: AppId,
        path_prefix: String,
        read_only: bool,
    },
    CheckFileAccess {
        app_id: AppId,
        path: String,
        write: bool,
    },
    ListFileGrants {
        app_id: AppId,
    },
    SetAccessibilityTree {
        app_id: AppId,
        nodes: Vec<AccessibilityNode>,
    },
    GetAccessibilityTree {
        app_id: AppId,
    },
    SetAccessibilityProfile {
        profile: AccessibilityProfile,
    },
    GetAccessibilityProfile,
    RecordAccessibilityEvent {
        event: AccessibilityEvent,
    },
    ListAccessibilityEvents {
        max_items: usize,
    },
    ClearAccessibilityEvents,
    PushCaptionEvent {
        event: CaptionEvent,
    },
    ListCaptionEvents {
        max_items: usize,
    },
    ClearCaptionEvents,
    GetSpeechState {
        max_items: usize,
    },
    SetLocale {
        locale: String,
    },
    GetLocale,
    SetSpeechVoice {
        voice_id: Option<String>,
    },
    ListSpeechVoices,
    GetSpeechOutputStatus,
    TickSpeechLane {
        now_ns: u64,
    },
    AdvanceSpeechLane,
    ClearSpeechLane,
    GetAccessibilityFocus,
    NoteNotification {
        app_id: AppId,
    },
    ClearNotifications {
        app_id: Option<AppId>,
    },
    SetWorkspace {
        workspace_id: WorkspaceId,
    },
    GetWorkspace,
    SetWorkspaceLayout {
        workspace_id: WorkspaceId,
        layout: WorkspaceLayout,
    },
    GetWorkspaceLayout {
        workspace_id: WorkspaceId,
    },
    SetWorkspaceRule {
        workspace_id: WorkspaceId,
        rule: WorkspaceRule,
    },
    GetWorkspaceRule {
        workspace_id: WorkspaceId,
    },
    ToggleScratchpad,
    ToggleOverview,
    SetPowerState {
        power_state: SessionPowerState,
    },
    SetDisplayProfileState {
        profile: DisplayProfile,
    },
    GetDisplayProfileState,
    SetClipboardHistoryLen {
        len: u32,
    },
    SetShellDensity {
        profile: ShellDensityProfile,
    },
    GetShellDensity,
    SetMotionProfile {
        profile: MotionProfile,
    },
    GetMotionProfile,
    SetRestoreDisposition {
        disposition: RestoreDisposition,
    },
    GetRestoreDisposition,
    SetStageSets {
        sets: Vec<StageSet>,
    },
    GetStageSets,
    SetStageSetPolicy {
        policy: StageSetPolicy,
    },
    GetStageSetPolicy,
    SetWindowRules {
        rules: Vec<WindowRule>,
    },
    GetWindowRules,
    GetSessionSnapshot,
    ListApps,
}

#[derive(Clone, Debug)]
pub enum ShellResponse {
    Ack,
    Apps(Vec<ShellAppEntry>),
    Workspace(WorkspaceId),
    WorkspaceLayout(WorkspaceLayout),
    WorkspaceRule(WorkspaceRule),
    ToggleState(bool),
    Permission(PermissionState),
    Permissions(Vec<PermissionEntry>),
    FileAccess(bool),
    FileGrants(Vec<FileGrant>),
    AccessibilityTree(Vec<AccessibilityNode>),
    AccessibilityProfile(AccessibilityProfile),
    AccessibilityEvents(Vec<AccessibilityEvent>),
    CaptionEvents(Vec<CaptionEvent>),
    SpeechState(SpeechState),
    Locale(String),
    SpeechVoices(Vec<crate::audio::tts::VoiceCatalogEntry>),
    SpeechOutputStatus(SpeechOutputStatus),
    AccessibilityFocus(Option<AccessibilityFocusState>),
    DisplayProfile(DisplayProfile),
    ShellDensity(ShellDensityProfile),
    MotionProfile(MotionProfile),
    RestoreDisposition(RestoreDisposition),
    StageSets(Vec<StageSet>),
    StageSetPolicy(StageSetPolicy),
    WindowRules(Vec<WindowRule>),
    SessionSnapshot(SessionSnapshot),
    Error(String),
}

pub struct EchShell {
    running: AtomicBool,
    workspace_id: Mutex<WorkspaceId>,
    power_state: Mutex<SessionPowerState>,
    workspace_layouts: Mutex<BTreeMap<WorkspaceId, WorkspaceLayout>>,
    workspace_rules: Mutex<BTreeMap<WorkspaceId, WorkspaceRule>>,
    overview_active: AtomicBool,
    scratchpad_visible: AtomicBool,
    unread_notifications: Mutex<u32>,
    clipboard_history_len: Mutex<u32>,
    apps: Mutex<BTreeMap<AppId, ShellAppEntry>>,
    permissions: Mutex<BTreeMap<AppId, BTreeMap<DesktopPermission, PermissionState>>>,
    file_grants: Mutex<BTreeMap<AppId, Vec<FileGrant>>>,
    accessibility: Mutex<BTreeMap<AppId, Vec<AccessibilityNode>>>,
    accessibility_profile: Mutex<AccessibilityProfile>,
    accessibility_events: Mutex<Vec<AccessibilityEvent>>,
    caption_events: Mutex<Vec<CaptionEvent>>,
    speech_lane: Mutex<SpeechLane>,
    speech_output: Mutex<SpeechOutputState>,
    accessibility_focus: Mutex<Option<AccessibilityFocusState>>,
    display_profile: Mutex<DisplayProfile>,
    shell_density: Mutex<ShellDensityProfile>,
    motion_profile: Mutex<MotionProfile>,
    restore_state: Mutex<RestoreDisposition>,
    stage_sets: Mutex<Vec<StageSet>>,
    stage_set_policy: Mutex<StageSetPolicy>,
    window_rules: Mutex<Vec<WindowRule>>,
    command_queue: MailboxRing<ShellCommand>,
    response_queue: MailboxRing<ShellResponse>,
}

impl EchShell {
    pub fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            workspace_id: Mutex::new(0),
            power_state: Mutex::new(SessionPowerState::Active),
            workspace_layouts: Mutex::new(BTreeMap::new()),
            workspace_rules: Mutex::new(BTreeMap::new()),
            overview_active: AtomicBool::new(false),
            scratchpad_visible: AtomicBool::new(false),
            unread_notifications: Mutex::new(0),
            clipboard_history_len: Mutex::new(0),
            apps: Mutex::new(BTreeMap::new()),
            permissions: Mutex::new(BTreeMap::new()),
            file_grants: Mutex::new(BTreeMap::new()),
            accessibility: Mutex::new(BTreeMap::new()),
            accessibility_profile: Mutex::new(AccessibilityProfile::default()),
            accessibility_events: Mutex::new(Vec::new()),
            caption_events: Mutex::new(Vec::new()),
            speech_lane: Mutex::new(SpeechLane::default()),
            speech_output: Mutex::new(SpeechOutputState::default()),
            accessibility_focus: Mutex::new(None),
            display_profile: Mutex::new(DisplayProfile::default()),
            shell_density: Mutex::new(ShellDensityProfile::Balanced),
            motion_profile: Mutex::new(MotionProfile::Standard),
            restore_state: Mutex::new(RestoreDisposition::RestoreIfClean),
            stage_sets: Mutex::new(Vec::new()),
            stage_set_policy: Mutex::new(StageSetPolicy::default()),
            window_rules: Mutex::new(Vec::new()),
            command_queue: MailboxRing::with_capacity_pow2(SHELL_COMMAND_QUEUE_CAPACITY),
            response_queue: MailboxRing::with_capacity_pow2(SHELL_RESPONSE_QUEUE_CAPACITY),
        }
    }

    pub fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
        crate::serial_println!("[ECHSHELL] service started");
    }

    pub fn send_command(&self, command: ShellCommand) -> bool {
        self.command_queue.try_push(command).is_ok()
    }

    pub fn receive_response(&self) -> Option<ShellResponse> {
        self.response_queue.pop()
    }

    fn queue_accessibility_event(&self, event: AccessibilityEvent) {
        let mut events = self.accessibility_events.lock();
        if events.last() == Some(&event) {
            return;
        }
        if events.len() >= ACCESSIBILITY_EVENT_CAPACITY {
            events.remove(0);
        }
        events.push(event.clone());
        drop(events);

        let mut captions = self.caption_events.lock();
        let spoken_text = event.label.clone();
        let caption = CaptionEvent {
            app_id: event.app_id,
            source_label: match event.kind {
                AccessibilityEventKind::FocusChanged => String::from("Focus"),
                AccessibilityEventKind::WindowOpened => String::from("Window"),
                AccessibilityEventKind::WindowClosed => String::from("Window"),
                AccessibilityEventKind::DialogOpened => String::from("Dialog"),
                AccessibilityEventKind::SelectionChanged => String::from("Selection"),
                AccessibilityEventKind::NotificationPosted => String::from("Notification"),
                AccessibilityEventKind::ValueChanged => String::from("Value"),
                AccessibilityEventKind::LiveRegionChanged => String::from("Live Region"),
            },
            text: event.label,
        };
        self.push_caption_locked(&mut captions, caption);
        self.queue_speech_event(SpeechEvent {
            app_id: event.app_id,
            source_label: match event.kind {
                AccessibilityEventKind::FocusChanged => String::from("Focus"),
                AccessibilityEventKind::WindowOpened => String::from("Window"),
                AccessibilityEventKind::WindowClosed => String::from("Window"),
                AccessibilityEventKind::DialogOpened => String::from("Dialog"),
                AccessibilityEventKind::SelectionChanged => String::from("Selection"),
                AccessibilityEventKind::NotificationPosted => String::from("Notification"),
                AccessibilityEventKind::ValueChanged => String::from("Value"),
                AccessibilityEventKind::LiveRegionChanged => String::from("Live"),
            },
            text: spoken_text,
        });
    }

    fn push_caption_locked(&self, captions: &mut Vec<CaptionEvent>, event: CaptionEvent) {
        if captions.last() == Some(&event) {
            return;
        }
        if captions.len() >= CAPTION_EVENT_CAPACITY {
            captions.remove(0);
        }
        captions.push(event);
    }

    fn queue_caption_event(&self, event: CaptionEvent) {
        let mut captions = self.caption_events.lock();
        self.push_caption_locked(&mut captions, event);
    }

    fn queue_speech_event(&self, event: SpeechEvent) {
        if !self.accessibility_profile.lock().screen_reader {
            return;
        }

        let mut speak_now = None;
        let mut speech = self.speech_lane.lock();
        if speech.active.as_ref() == Some(&event) {
            speech.coalesced_count = speech.coalesced_count.saturating_add(1);
            return;
        }
        if speech.pending.last() == Some(&event) {
            speech.coalesced_count = speech.coalesced_count.saturating_add(1);
            return;
        }
        if let Some(last) = speech.pending.last_mut() {
            if last.source_label == event.source_label {
                *last = event.clone();
                speech.coalesced_count = speech.coalesced_count.saturating_add(1);
                return;
            }
        }
        if speech.active.is_none() {
            speech.active = Some(event.clone());
            speak_now = Some(event);
        } else {
            if speech.pending.len() >= SPEECH_EVENT_CAPACITY {
                speech.pending.remove(0);
                speech.dropped_count = speech.dropped_count.saturating_add(1);
            }
            speech.pending.push(event);
        }
        drop(speech);
        if let Some(event) = speak_now {
            self.play_speech_event(&event);
        }
    }

    fn speech_state(&self, max_items: usize) -> SpeechState {
        self.speech_lane.lock().state(max_items)
    }

    fn advance_speech_lane(&self) -> SpeechState {
        let mut speech = self.speech_lane.lock();
        speech.active = if speech.pending.is_empty() {
            None
        } else {
            Some(speech.pending.remove(0))
        };
        speech.active_deadline_ns = 0;
        let active = speech.active.clone();
        let state = speech.state(SPEECH_EVENT_CAPACITY);
        drop(speech);
        if let Some(event) = active {
            self.play_speech_event(&event);
        }
        state
    }

    fn tick_speech_lane(&self, now_ns: u64) -> SpeechState {
        let mut speak_now = None;
        {
            let mut speech = self.speech_lane.lock();
            let should_advance = speech.active.is_none()
                || (speech.active_deadline_ns != 0 && now_ns >= speech.active_deadline_ns);
            if should_advance {
                speech.active = if speech.pending.is_empty() {
                    None
                } else {
                    Some(speech.pending.remove(0))
                };
                speech.active_deadline_ns = 0;
                speak_now = speech.active.clone();
            } else if speech.active_deadline_ns == 0 && self.speech_retry_ready(now_ns) {
                speak_now = speech.active.clone();
            }
        }
        if let Some(event) = speak_now {
            self.play_speech_event_at_time(&event, now_ns);
        }
        self.speech_state(SPEECH_EVENT_CAPACITY)
    }

    fn play_speech_event(&self, event: &SpeechEvent) {
        let now = crate::gui::animation::get_time_ns();
        self.play_speech_event_at_time(event, now);
    }

    fn play_speech_event_at_time(&self, event: &SpeechEvent, now_ns: u64) {
        let (locale, preferred_voice_id) = {
            let output = self.speech_output.lock();
            (
                output.status.locale.clone(),
                output.status.preferred_voice_id.clone(),
            )
        };
        let voice =
            match crate::audio::tts::select_voice(locale.as_str(), preferred_voice_id.as_deref()) {
                Ok(voice) => voice,
                Err(error) => {
                    self.record_speech_failure(
                        SpeechOutputError::from_voice_error(error),
                        None,
                        now_ns,
                    );
                    return;
                }
            };
        let clip = crate::audio::tts::synthesize_with_voice(event.text.as_str(), voice);
        if clip.pcm16_le.is_empty() {
            return;
        }
        let duration_ns = clip.duration_ns;
        let channel_id = match self.ensure_speech_channel(clip.sample_rate, clip.channels, now_ns) {
            Ok(channel_id) => channel_id,
            Err(error) => {
                self.record_speech_failure(error, None, now_ns);
                return;
            }
        };
        match crate::ipc::request_audio_sync(
            event.app_id,
            crate::services::AudioCommand::SendAudioData {
                channel_id,
                data: clip.pcm16_le,
            },
        ) {
            Some(crate::services::AudioResponse::Success) => {
                self.record_speech_success(channel_id, voice.id);
            }
            Some(crate::services::AudioResponse::Error(error)) => {
                self.record_speech_failure(
                    SpeechOutputError::from_audio_error(error),
                    Some(channel_id),
                    now_ns,
                );
                return;
            }
            Some(other) => {
                self.record_speech_failure(
                    SpeechOutputError {
                        kind: SpeechOutputErrorKind::AudioServiceUnavailable,
                        detail: alloc::format!("unexpected audio response: {:?}", other),
                        retryable: true,
                    },
                    Some(channel_id),
                    now_ns,
                );
                return;
            }
            None => {
                self.record_speech_failure(
                    SpeechOutputError {
                        kind: SpeechOutputErrorKind::AudioServiceUnavailable,
                        detail: String::from("audio service unavailable"),
                        retryable: true,
                    },
                    Some(channel_id),
                    now_ns,
                );
                return;
            }
        }
        let mut speech = self.speech_lane.lock();
        if speech.active.as_ref() == Some(event) {
            speech.active_deadline_ns = now_ns
                .saturating_add(duration_ns)
                .saturating_add(60_000_000);
        }
    }

    fn speech_retry_ready(&self, now_ns: u64) -> bool {
        let output = self.speech_output.lock();
        output.status.retry_after_ns == 0 || now_ns >= output.status.retry_after_ns
    }

    fn record_speech_success(&self, channel_id: u32, resolved_voice_id: &'static str) {
        let mut output = self.speech_output.lock();
        output.status.channel_id = Some(channel_id);
        output.status.health = SpeechOutputHealth::Ready;
        output.status.retry_after_ns = 0;
        output.status.resolved_voice_id = Some(String::from(resolved_voice_id));
        output.status.last_error = None;
    }

    fn record_speech_failure(
        &self,
        error: SpeechOutputError,
        channel_id: Option<u32>,
        now_ns: u64,
    ) {
        let mut output = self.speech_output.lock();
        output.status.failure_count = output.status.failure_count.saturating_add(1);
        output.status.last_error = Some(error.clone());
        if channel_id.is_some() || matches!(error.kind, SpeechOutputErrorKind::AudioInvalidChannel)
        {
            output.status.channel_id = None;
        }
        if error.retryable {
            output.status.health = SpeechOutputHealth::Recovering;
            output.status.retry_after_ns = now_ns.saturating_add(SPEECH_RETRY_BACKOFF_NS);
        } else {
            output.status.health = SpeechOutputHealth::FailedClosed;
            output.status.retry_after_ns = 0;
        }
    }

    fn ensure_speech_channel(
        &self,
        sample_rate: u32,
        channels: u8,
        now_ns: u64,
    ) -> Result<u32, SpeechOutputError> {
        if let Some(channel_id) = self.speech_output.lock().status.channel_id {
            return Ok(channel_id);
        }
        let response = crate::ipc::request_audio_sync(
            0,
            crate::services::AudioCommand::CreateChannel {
                format: crate::services::AudioFormat::PCM16,
                sample_rate,
                channels,
            },
        )
        .ok_or_else(|| SpeechOutputError {
            kind: SpeechOutputErrorKind::AudioServiceUnavailable,
            detail: String::from("audio service unavailable"),
            retryable: true,
        })?;
        let channel_id = match response {
            crate::services::AudioResponse::ChannelCreated { channel_id } => channel_id,
            crate::services::AudioResponse::Error(error) => {
                return Err(SpeechOutputError::from_audio_error(error));
            }
            other => {
                return Err(SpeechOutputError {
                    kind: SpeechOutputErrorKind::AudioServiceUnavailable,
                    detail: alloc::format!("unexpected audio response: {:?}", other),
                    retryable: true,
                });
            }
        };
        match crate::ipc::request_audio_sync(
            0,
            crate::services::AudioCommand::SetVolume {
                channel_id,
                volume: 0.92,
            },
        ) {
            Some(crate::services::AudioResponse::Success) => {}
            Some(crate::services::AudioResponse::Error(error)) => {
                return Err(SpeechOutputError::from_audio_error(error));
            }
            Some(other) => {
                return Err(SpeechOutputError {
                    kind: SpeechOutputErrorKind::AudioServiceUnavailable,
                    detail: alloc::format!("unexpected audio response: {:?}", other),
                    retryable: true,
                });
            }
            None => {
                return Err(SpeechOutputError {
                    kind: SpeechOutputErrorKind::AudioServiceUnavailable,
                    detail: String::from("audio service unavailable"),
                    retryable: true,
                });
            }
        }
        self.speech_output.lock().status.channel_id = Some(channel_id);
        Ok(channel_id)
    }

    fn focused_node_state(
        &self,
        app_id: AppId,
        window_id: Option<WindowId>,
        nodes: &[AccessibilityNode],
    ) -> Option<AccessibilityFocusState> {
        let focused = nodes.iter().find(|node| node.focused)?;
        Some(AccessibilityFocusState {
            app_id,
            window_id,
            node_id: focused.id,
            role: focused.role,
            label: focused.label.clone(),
            description: focused.description.clone(),
            bounds: focused.bounds,
        })
    }

    fn publish_focus_state(&self, next_focus: Option<AccessibilityFocusState>) {
        let mut focus = self.accessibility_focus.lock();
        let previous = focus.clone();
        if previous == next_focus {
            return;
        }
        *focus = next_focus.clone();
        drop(focus);

        if let Some(next) = next_focus {
            self.queue_accessibility_event(AccessibilityEvent {
                app_id: next.app_id,
                window_id: next.window_id,
                node_id: Some(next.node_id),
                kind: AccessibilityEventKind::FocusChanged,
                label: next.label.clone(),
            });
            if next.role == AccessibilityRole::Dialog {
                self.queue_accessibility_event(AccessibilityEvent {
                    app_id: next.app_id,
                    window_id: next.window_id,
                    node_id: Some(next.node_id),
                    kind: AccessibilityEventKind::DialogOpened,
                    label: next.label,
                });
            } else if next.role == AccessibilityRole::ListItem {
                self.queue_accessibility_event(AccessibilityEvent {
                    app_id: next.app_id,
                    window_id: next.window_id,
                    node_id: Some(next.node_id),
                    kind: AccessibilityEventKind::SelectionChanged,
                    label: next.label,
                });
            }
        }
    }

    fn tail_accessibility_events(&self, max_items: usize) -> Vec<AccessibilityEvent> {
        let events = self.accessibility_events.lock();
        let start = events.len().saturating_sub(max_items.max(1));
        events[start..].to_vec()
    }

    fn tail_caption_events(&self, max_items: usize) -> Vec<CaptionEvent> {
        let captions = self.caption_events.lock();
        let start = captions.len().saturating_sub(max_items.max(1));
        captions[start..].to_vec()
    }

    pub fn process_command(&self, command: ShellCommand) -> ShellResponse {
        match command {
            ShellCommand::RegisterApp { app_id, name } => {
                let workspace_id = *self.workspace_id.lock();
                self.apps.lock().insert(
                    app_id,
                    ShellAppEntry {
                        app_id,
                        name,
                        window_id: None,
                        visible: false,
                        focused: false,
                        workspace_id,
                        running: false,
                        health: AppHealth::Idle,
                        launch_count: 0,
                        crash_count: 0,
                        needs_attention: false,
                        status_line: String::from("registered"),
                        auto_restore: false,
                    },
                );
                self.ensure_permission_defaults(app_id);
                ShellResponse::Ack
            }
            ShellCommand::UnregisterApp { app_id } => {
                self.apps.lock().remove(&app_id);
                self.permissions.lock().remove(&app_id);
                self.file_grants.lock().remove(&app_id);
                self.accessibility.lock().remove(&app_id);
                if self
                    .accessibility_focus
                    .lock()
                    .as_ref()
                    .map(|focus| focus.app_id == app_id)
                    .unwrap_or(false)
                {
                    *self.accessibility_focus.lock() = None;
                }
                ShellResponse::Ack
            }
            ShellCommand::UpdateAppWindow {
                app_id,
                window_id,
                visible,
                focused,
                workspace_id,
            } => {
                let mut apps = self.apps.lock();
                let Some(entry) = apps.get_mut(&app_id) else {
                    return ShellResponse::Error(String::from("app not registered"));
                };
                let previous_visible = entry.visible;
                let previous_focused = entry.focused;
                let previous_window = entry.window_id;
                let label = entry.name.clone();
                entry.window_id = window_id;
                entry.visible = visible;
                entry.focused = focused;
                entry.workspace_id = workspace_id;
                if window_id.is_some() && visible {
                    entry.running = true;
                    if entry.health != AppHealth::Crashed {
                        entry.health = AppHealth::Running;
                    }
                }
                if focused {
                    entry.needs_attention = false;
                    if entry.health == AppHealth::Attention {
                        entry.health = AppHealth::Running;
                    }
                }
                let current_focus = if focused {
                    self.accessibility
                        .lock()
                        .get(&app_id)
                        .and_then(|nodes| self.focused_node_state(app_id, window_id, nodes))
                } else {
                    None
                };
                drop(apps);

                if !previous_visible && visible {
                    self.queue_accessibility_event(AccessibilityEvent {
                        app_id,
                        window_id,
                        node_id: None,
                        kind: AccessibilityEventKind::WindowOpened,
                        label: label.clone(),
                    });
                } else if previous_visible && !visible {
                    self.queue_accessibility_event(AccessibilityEvent {
                        app_id,
                        window_id: previous_window,
                        node_id: None,
                        kind: AccessibilityEventKind::WindowClosed,
                        label: label.clone(),
                    });
                }
                if focused && !previous_focused {
                    if let Some(focus_state) = current_focus {
                        self.publish_focus_state(Some(focus_state));
                    } else {
                        self.queue_accessibility_event(AccessibilityEvent {
                            app_id,
                            window_id,
                            node_id: None,
                            kind: AccessibilityEventKind::FocusChanged,
                            label,
                        });
                    }
                } else if previous_focused && !focused {
                    let focused_app = self
                        .accessibility_focus
                        .lock()
                        .as_ref()
                        .map(|focus| focus.app_id == app_id)
                        .unwrap_or(false);
                    if focused_app {
                        *self.accessibility_focus.lock() = None;
                    }
                }
                ShellResponse::Ack
            }
            ShellCommand::MarkAppLaunch {
                app_id,
                status_line,
            } => {
                let mut apps = self.apps.lock();
                let Some(entry) = apps.get_mut(&app_id) else {
                    return ShellResponse::Error(String::from("app not registered"));
                };
                entry.running = true;
                entry.health = AppHealth::Running;
                entry.needs_attention = false;
                entry.launch_count = entry.launch_count.saturating_add(1);
                entry.status_line = status_line;
                ShellResponse::Ack
            }
            ShellCommand::MarkAppExit {
                app_id,
                clean,
                status_line,
            } => {
                let mut apps = self.apps.lock();
                let Some(entry) = apps.get_mut(&app_id) else {
                    return ShellResponse::Error(String::from("app not registered"));
                };
                let label = entry.name.clone();
                let window_id = entry.window_id;
                let was_visible = entry.visible;
                entry.running = false;
                entry.visible = false;
                entry.focused = false;
                entry.window_id = None;
                if clean {
                    entry.health = AppHealth::Idle;
                    entry.needs_attention = false;
                } else {
                    entry.health = AppHealth::Crashed;
                    entry.needs_attention = true;
                    entry.crash_count = entry.crash_count.saturating_add(1);
                }
                entry.status_line = status_line;
                drop(apps);
                if was_visible {
                    self.queue_accessibility_event(AccessibilityEvent {
                        app_id,
                        window_id,
                        node_id: None,
                        kind: AccessibilityEventKind::WindowClosed,
                        label,
                    });
                }
                ShellResponse::Ack
            }
            ShellCommand::RecordAppFault { app_id, detail } => {
                let mut apps = self.apps.lock();
                let Some(entry) = apps.get_mut(&app_id) else {
                    return ShellResponse::Error(String::from("app not registered"));
                };
                let label = entry.name.clone();
                let window_id = entry.window_id;
                let was_visible = entry.visible;
                entry.running = false;
                entry.health = AppHealth::Crashed;
                entry.needs_attention = true;
                entry.visible = false;
                entry.focused = false;
                entry.window_id = None;
                entry.crash_count = entry.crash_count.saturating_add(1);
                entry.status_line = detail;
                drop(apps);
                if was_visible {
                    self.queue_accessibility_event(AccessibilityEvent {
                        app_id,
                        window_id,
                        node_id: None,
                        kind: AccessibilityEventKind::WindowClosed,
                        label,
                    });
                }
                ShellResponse::Ack
            }
            ShellCommand::ClearAppAttention {
                app_id,
                status_line,
            } => {
                let mut apps = self.apps.lock();
                let Some(entry) = apps.get_mut(&app_id) else {
                    return ShellResponse::Error(String::from("app not registered"));
                };
                entry.needs_attention = false;
                if entry.running {
                    entry.health = AppHealth::Running;
                } else if entry.health != AppHealth::Crashed {
                    entry.health = AppHealth::Idle;
                }
                if let Some(status_line) = status_line {
                    entry.status_line = status_line;
                }
                ShellResponse::Ack
            }
            ShellCommand::SetAutoRestore { app_id, enabled } => {
                let mut apps = self.apps.lock();
                let Some(entry) = apps.get_mut(&app_id) else {
                    return ShellResponse::Error(String::from("app not registered"));
                };
                entry.auto_restore = enabled;
                ShellResponse::Ack
            }
            ShellCommand::SetPermission {
                app_id,
                permission,
                state,
            } => {
                self.ensure_permission_defaults(app_id);
                self.permissions
                    .lock()
                    .entry(app_id)
                    .or_default()
                    .insert(permission, state);
                ShellResponse::Ack
            }
            ShellCommand::GetPermission { app_id, permission } => {
                self.ensure_permission_defaults(app_id);
                let state = self
                    .permissions
                    .lock()
                    .get(&app_id)
                    .and_then(|entries| entries.get(&permission).copied())
                    .unwrap_or(PermissionState::Ask);
                ShellResponse::Permission(state)
            }
            ShellCommand::ListPermissions { app_id } => {
                self.ensure_permission_defaults(app_id);
                let entries = self
                    .permissions
                    .lock()
                    .get(&app_id)
                    .map(|entries| {
                        entries
                            .iter()
                            .map(|(permission, state)| PermissionEntry {
                                permission: *permission,
                                state: *state,
                            })
                            .collect()
                    })
                    .unwrap_or_else(Vec::new);
                ShellResponse::Permissions(entries)
            }
            ShellCommand::GrantFileAccess {
                app_id,
                path_prefix,
                read_only,
            } => {
                let mut grants = self.file_grants.lock();
                let entries = grants.entry(app_id).or_default();
                if !entries.iter().any(|grant| grant.path_prefix == path_prefix) {
                    entries.push(FileGrant {
                        path_prefix,
                        read_only,
                    });
                }
                ShellResponse::Ack
            }
            ShellCommand::CheckFileAccess {
                app_id,
                path,
                write,
            } => {
                let granted = self
                    .file_grants
                    .lock()
                    .get(&app_id)
                    .map(|grants| {
                        grants.iter().any(|grant| {
                            path.starts_with(&grant.path_prefix) && (!write || !grant.read_only)
                        })
                    })
                    .unwrap_or(false);
                ShellResponse::FileAccess(granted)
            }
            ShellCommand::ListFileGrants { app_id } => {
                let grants = self
                    .file_grants
                    .lock()
                    .get(&app_id)
                    .cloned()
                    .unwrap_or_else(Vec::new);
                ShellResponse::FileGrants(grants)
            }
            ShellCommand::SetAccessibilityTree { app_id, nodes } => {
                let previous_nodes = self
                    .accessibility
                    .lock()
                    .insert(app_id, nodes.clone())
                    .unwrap_or_else(Vec::new);
                crate::services::at_spi::get_bridge().publish_tree(app_id, &nodes);
                let window_id = self
                    .apps
                    .lock()
                    .get(&app_id)
                    .and_then(|entry| entry.window_id);
                let previous_focus = self.focused_node_state(app_id, window_id, &previous_nodes);
                let next_focus = self.focused_node_state(app_id, window_id, &nodes);
                if let (Some(previous), Some(next)) = (previous_focus.clone(), next_focus.clone()) {
                    if previous.node_id == next.node_id
                        && previous.window_id == next.window_id
                        && previous.label != next.label
                    {
                        self.queue_accessibility_event(AccessibilityEvent {
                            app_id,
                            window_id: next.window_id,
                            node_id: Some(next.node_id),
                            kind: AccessibilityEventKind::ValueChanged,
                            label: next.label.clone(),
                        });
                    }
                }
                self.publish_focus_state(next_focus);
                ShellResponse::Ack
            }
            ShellCommand::GetAccessibilityTree { app_id } => {
                let nodes = self
                    .accessibility
                    .lock()
                    .get(&app_id)
                    .cloned()
                    .unwrap_or_else(Vec::new);
                ShellResponse::AccessibilityTree(nodes)
            }
            ShellCommand::SetAccessibilityProfile { profile } => {
                *self.accessibility_profile.lock() = profile;
                if !profile.screen_reader {
                    let mut speech = self.speech_lane.lock();
                    speech.active = None;
                    speech.pending.clear();
                    speech.active_deadline_ns = 0;
                }
                ShellResponse::Ack
            }
            ShellCommand::GetAccessibilityProfile => {
                ShellResponse::AccessibilityProfile(*self.accessibility_profile.lock())
            }
            ShellCommand::RecordAccessibilityEvent { event } => {
                self.queue_accessibility_event(event);
                ShellResponse::Ack
            }
            ShellCommand::ListAccessibilityEvents { max_items } => {
                ShellResponse::AccessibilityEvents(self.tail_accessibility_events(max_items))
            }
            ShellCommand::ClearAccessibilityEvents => {
                self.accessibility_events.lock().clear();
                ShellResponse::Ack
            }
            ShellCommand::PushCaptionEvent { event } => {
                self.queue_caption_event(event);
                ShellResponse::Ack
            }
            ShellCommand::ListCaptionEvents { max_items } => {
                ShellResponse::CaptionEvents(self.tail_caption_events(max_items))
            }
            ShellCommand::ClearCaptionEvents => {
                self.caption_events.lock().clear();
                ShellResponse::Ack
            }
            ShellCommand::GetSpeechState { max_items } => {
                ShellResponse::SpeechState(self.speech_state(max_items))
            }
            ShellCommand::SetLocale { locale } => {
                self.speech_output.lock().status.locale = locale;
                ShellResponse::Ack
            }
            ShellCommand::GetLocale => {
                ShellResponse::Locale(self.speech_output.lock().status.locale.clone())
            }
            ShellCommand::SetSpeechVoice { voice_id } => {
                let mut output = self.speech_output.lock();
                output.status.preferred_voice_id = voice_id;
                output.status.resolved_voice_id = None;
                output.status.last_error = None;
                output.status.health = SpeechOutputHealth::Ready;
                output.status.retry_after_ns = 0;
                ShellResponse::Ack
            }
            ShellCommand::ListSpeechVoices => {
                ShellResponse::SpeechVoices(crate::audio::tts::voice_catalog())
            }
            ShellCommand::GetSpeechOutputStatus => {
                ShellResponse::SpeechOutputStatus(self.speech_output.lock().snapshot())
            }
            ShellCommand::TickSpeechLane { now_ns } => {
                ShellResponse::SpeechState(self.tick_speech_lane(now_ns))
            }
            ShellCommand::AdvanceSpeechLane => {
                ShellResponse::SpeechState(self.advance_speech_lane())
            }
            ShellCommand::ClearSpeechLane => {
                let mut speech = self.speech_lane.lock();
                speech.active = None;
                speech.pending.clear();
                speech.active_deadline_ns = 0;
                ShellResponse::Ack
            }
            ShellCommand::GetAccessibilityFocus => {
                ShellResponse::AccessibilityFocus(self.accessibility_focus.lock().clone())
            }
            ShellCommand::NoteNotification { app_id } => {
                let mut unread = self.unread_notifications.lock();
                *unread = unread.saturating_add(1);
                let mut label = String::from("Notification");
                if let Some(entry) = self.apps.lock().get_mut(&app_id) {
                    label = entry.name.clone();
                    entry.needs_attention = true;
                    if entry.health != AppHealth::Crashed {
                        entry.health = AppHealth::Attention;
                    }
                }
                self.queue_accessibility_event(AccessibilityEvent {
                    app_id,
                    window_id: self
                        .apps
                        .lock()
                        .get(&app_id)
                        .and_then(|entry| entry.window_id),
                    node_id: None,
                    kind: AccessibilityEventKind::NotificationPosted,
                    label,
                });
                ShellResponse::Ack
            }
            ShellCommand::ClearNotifications { app_id } => {
                *self.unread_notifications.lock() = 0;
                if let Some(app_id) = app_id {
                    if let Some(entry) = self.apps.lock().get_mut(&app_id) {
                        entry.needs_attention = false;
                        if entry.health == AppHealth::Attention {
                            entry.health = if entry.running {
                                AppHealth::Running
                            } else {
                                AppHealth::Idle
                            };
                        }
                    }
                } else {
                    for entry in self.apps.lock().values_mut() {
                        entry.needs_attention = false;
                        if entry.health == AppHealth::Attention {
                            entry.health = if entry.running {
                                AppHealth::Running
                            } else {
                                AppHealth::Idle
                            };
                        }
                    }
                }
                ShellResponse::Ack
            }
            ShellCommand::SetWorkspace { workspace_id } => {
                *self.workspace_id.lock() = workspace_id;
                ShellResponse::Ack
            }
            ShellCommand::GetWorkspace => ShellResponse::Workspace(*self.workspace_id.lock()),
            ShellCommand::SetWorkspaceLayout {
                workspace_id,
                layout,
            } => {
                self.workspace_layouts.lock().insert(workspace_id, layout);
                ShellResponse::Ack
            }
            ShellCommand::GetWorkspaceLayout { workspace_id } => {
                let layout = self
                    .workspace_layouts
                    .lock()
                    .get(&workspace_id)
                    .copied()
                    .unwrap_or(WorkspaceLayout::Dwindle);
                ShellResponse::WorkspaceLayout(layout)
            }
            ShellCommand::SetWorkspaceRule { workspace_id, rule } => {
                self.workspace_rules.lock().insert(workspace_id, rule);
                ShellResponse::Ack
            }
            ShellCommand::GetWorkspaceRule { workspace_id } => {
                let rule = self
                    .workspace_rules
                    .lock()
                    .get(&workspace_id)
                    .copied()
                    .unwrap_or_else(WorkspaceRule::default);
                ShellResponse::WorkspaceRule(rule)
            }
            ShellCommand::ToggleScratchpad => {
                let next = !self.scratchpad_visible.load(Ordering::Acquire);
                self.scratchpad_visible.store(next, Ordering::Release);
                ShellResponse::ToggleState(next)
            }
            ShellCommand::ToggleOverview => {
                let next = !self.overview_active.load(Ordering::Acquire);
                self.overview_active.store(next, Ordering::Release);
                ShellResponse::ToggleState(next)
            }
            ShellCommand::SetPowerState { power_state } => {
                *self.power_state.lock() = power_state;
                ShellResponse::Ack
            }
            ShellCommand::SetDisplayProfileState { profile } => {
                *self.display_profile.lock() = profile;
                ShellResponse::Ack
            }
            ShellCommand::GetDisplayProfileState => {
                ShellResponse::DisplayProfile(self.display_profile.lock().clone())
            }
            ShellCommand::SetClipboardHistoryLen { len } => {
                *self.clipboard_history_len.lock() = len;
                ShellResponse::Ack
            }
            ShellCommand::SetShellDensity { profile } => {
                *self.shell_density.lock() = profile;
                ShellResponse::Ack
            }
            ShellCommand::GetShellDensity => {
                ShellResponse::ShellDensity(*self.shell_density.lock())
            }
            ShellCommand::SetMotionProfile { profile } => {
                *self.motion_profile.lock() = profile;
                ShellResponse::Ack
            }
            ShellCommand::GetMotionProfile => {
                ShellResponse::MotionProfile(*self.motion_profile.lock())
            }
            ShellCommand::SetRestoreDisposition { disposition } => {
                *self.restore_state.lock() = disposition;
                ShellResponse::Ack
            }
            ShellCommand::GetRestoreDisposition => {
                ShellResponse::RestoreDisposition(*self.restore_state.lock())
            }
            ShellCommand::SetStageSets { sets } => {
                *self.stage_sets.lock() = sets;
                ShellResponse::Ack
            }
            ShellCommand::GetStageSets => ShellResponse::StageSets(self.stage_sets.lock().clone()),
            ShellCommand::SetStageSetPolicy { policy } => {
                *self.stage_set_policy.lock() = policy;
                ShellResponse::Ack
            }
            ShellCommand::GetStageSetPolicy => {
                ShellResponse::StageSetPolicy(*self.stage_set_policy.lock())
            }
            ShellCommand::SetWindowRules { rules } => {
                *self.window_rules.lock() = rules;
                ShellResponse::Ack
            }
            ShellCommand::GetWindowRules => {
                ShellResponse::WindowRules(self.window_rules.lock().clone())
            }
            ShellCommand::GetSessionSnapshot => {
                let apps = self.apps.lock();
                let apps_running = apps.values().filter(|entry| entry.running).count() as u32;
                let apps_crashed = apps
                    .values()
                    .filter(|entry| entry.health == AppHealth::Crashed)
                    .count() as u32;
                let workspace_id = *self.workspace_id.lock();
                let workspace_layout = self
                    .workspace_layouts
                    .lock()
                    .get(&workspace_id)
                    .copied()
                    .unwrap_or(WorkspaceLayout::Dwindle);
                let power_state = *self.power_state.lock();
                let shell_state = if power_state == SessionPowerState::Locked {
                    crate::gui::protocol::ShellState::Locked
                } else if self.overview_active.load(Ordering::Acquire) {
                    crate::gui::protocol::ShellState::OverlayInteractive
                } else {
                    crate::gui::protocol::ShellState::DesktopReady
                };
                let accessibility_profile = *self.accessibility_profile.lock();
                let display_profile = self.display_profile.lock().clone();
                let locale = self.speech_output.lock().status.locale.clone();
                let output_scale = display_profile
                    .outputs
                    .iter()
                    .find(|output| output.output_id == display_profile.primary_output)
                    .map(|output| output.scale_100x as u32)
                    .unwrap_or(100);
                ShellResponse::SessionSnapshot(SessionSnapshot {
                    workspace_id,
                    workspace_layout,
                    power_state,
                    unread_notifications: *self.unread_notifications.lock(),
                    apps_running,
                    apps_crashed,
                    overview_active: self.overview_active.load(Ordering::Acquire),
                    scratchpad_visible: self.scratchpad_visible.load(Ordering::Acquire),
                    shell_ready: self.running.load(Ordering::Acquire),
                    boot_clean_desktop: apps_running == 0,
                    output_scale,
                    text_scale: accessibility_profile.text_scale_100x as u32,
                    clipboard_history_len: *self.clipboard_history_len.lock(),
                    accessibility_profile,
                    display_profile,
                    shell_density: *self.shell_density.lock(),
                    motion_profile: *self.motion_profile.lock(),
                    restore_state: *self.restore_state.lock(),
                    stage_set_policy: *self.stage_set_policy.lock(),
                    locale,
                    theme_variant: String::from("hybrid-titan"),
                    shell_state,
                })
            }
            ShellCommand::ListApps => {
                let apps = self.apps.lock().values().cloned().collect();
                ShellResponse::Apps(apps)
            }
        }
    }

    fn ensure_permission_defaults(&self, app_id: AppId) {
        let mut permissions = self.permissions.lock();
        let entries = permissions.entry(app_id).or_default();
        for permission in [
            DesktopPermission::ClipboardRead,
            DesktopPermission::ClipboardWrite,
            DesktopPermission::Notifications,
            DesktopPermission::FileDialogs,
            DesktopPermission::FileSystem,
            DesktopPermission::ScreenCapture,
        ] {
            entries.entry(permission).or_insert(PermissionState::Ask);
        }
    }

    pub fn run_service(&self) {
        while self.running.load(Ordering::SeqCst) {
            while let Some(command) = self.command_queue.pop() {
                let response = self.process_command(command);
                let _ = self.response_queue.push_overwrite(response);
            }

            for _ in 0..1000 {
                core::hint::spin_loop();
            }
        }
    }
}

lazy_static::lazy_static! {
    static ref ECH_SHELL: Arc<EchShell> = Arc::new(EchShell::new());
}

pub fn init() {
    ECH_SHELL.start();
    crate::serial_println!("[ECHSHELL] initialized");
}

pub fn get_shell_service() -> Arc<EchShell> {
    Arc::clone(&ECH_SHELL)
}

pub fn service_task() -> ! {
    let svc = get_shell_service();
    svc.run_service();
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::protocol::Rect;
    use crate::services::{AudioError, AudioErrorKind};

    #[test]
    fn accessibility_event_lane_is_bounded_and_feeds_captions() {
        let shell = EchShell::new();
        for index in 0..(ACCESSIBILITY_EVENT_CAPACITY + 8) {
            shell.queue_accessibility_event(AccessibilityEvent {
                app_id: 1,
                window_id: Some(7),
                node_id: Some(index as u64),
                kind: AccessibilityEventKind::FocusChanged,
                label: alloc::format!("focus-{}", index),
            });
        }

        let events = shell.tail_accessibility_events(ACCESSIBILITY_EVENT_CAPACITY + 8);
        let captions = shell.tail_caption_events(CAPTION_EVENT_CAPACITY + 8);
        assert_eq!(events.len(), ACCESSIBILITY_EVENT_CAPACITY);
        assert_eq!(captions.len(), CAPTION_EVENT_CAPACITY);
        assert_eq!(
            events.first().map(|event| event.label.as_str()),
            Some("focus-8")
        );
    }

    #[test]
    fn focus_publication_emits_dialog_event_and_tracks_focus_state() {
        let shell = EchShell::new();
        let focus = AccessibilityFocusState {
            app_id: 9,
            window_id: Some(44),
            node_id: 3,
            role: AccessibilityRole::Dialog,
            label: String::from("Quick Settings"),
            description: String::from("session toggles"),
            bounds: Rect::new(10, 20, 200, 100),
        };
        shell.publish_focus_state(Some(focus.clone()));

        let current = shell.accessibility_focus.lock().clone();
        let events = shell.tail_accessibility_events(4);
        assert_eq!(current, Some(focus));
        assert!(events
            .iter()
            .any(|event| event.kind == AccessibilityEventKind::FocusChanged));
        assert!(events
            .iter()
            .any(|event| event.kind == AccessibilityEventKind::DialogOpened));
    }

    #[test]
    fn speech_lane_coalesces_and_advances_without_unbounded_growth() {
        let shell = EchShell::new();
        *shell.accessibility_profile.lock() = AccessibilityProfile {
            screen_reader: true,
            magnifier_mode: MagnifierMode::Docked,
            ..AccessibilityProfile::default()
        };

        for index in 0..(SPEECH_EVENT_CAPACITY + 6) {
            shell.queue_speech_event(SpeechEvent {
                app_id: 1,
                source_label: String::from("Focus"),
                text: alloc::format!("Target {}", index),
            });
        }

        let state = shell.speech_state(SPEECH_EVENT_CAPACITY + 8);
        assert!(state.active.is_some());
        assert!(state.pending.len() <= SPEECH_EVENT_CAPACITY);

        let advanced = shell.advance_speech_lane();
        assert!(advanced.pending.len() <= SPEECH_EVENT_CAPACITY);
    }

    #[test]
    fn speech_output_status_tracks_locale_voice_catalog_and_preference() {
        let shell = EchShell::new();
        assert!(matches!(
            shell.process_command(ShellCommand::SetLocale {
                locale: String::from("en-GB"),
            }),
            ShellResponse::Ack
        ));
        assert!(matches!(
            shell.process_command(ShellCommand::SetSpeechVoice {
                voice_id: Some(String::from("gene")),
            }),
            ShellResponse::Ack
        ));

        let ShellResponse::Locale(locale) = shell.process_command(ShellCommand::GetLocale) else {
            unreachable!("locale response expected")
        };
        assert_eq!(locale, "en-GB");

        let ShellResponse::SpeechVoices(voices) =
            shell.process_command(ShellCommand::ListSpeechVoices)
        else {
            unreachable!("voice catalog response expected")
        };
        assert!(voices.iter().any(|voice| voice.id == "gene"));
        assert!(voices.iter().any(|voice| voice.id == "gene"
            && voice
                .supported_locales
                .iter()
                .any(|locale| locale == "en-gb")));

        let ShellResponse::SpeechOutputStatus(status) =
            shell.process_command(ShellCommand::GetSpeechOutputStatus)
        else {
            unreachable!("speech output status expected")
        };
        assert_eq!(status.locale, "en-GB");
        assert_eq!(status.preferred_voice_id.as_deref(), Some("gene"));
        assert_eq!(status.health, SpeechOutputHealth::Ready);
    }

    #[test]
    fn speech_output_failure_transitions_distinguish_retryable_audio_and_fail_closed_voice_errors()
    {
        let shell = EchShell::new();

        shell.record_speech_failure(
            SpeechOutputError::from_audio_error(AudioError {
                kind: AudioErrorKind::QueueSaturated,
                detail: String::from("queue full"),
                retryable: true,
            }),
            Some(7),
            1_000,
        );
        let recovering = shell.speech_output.lock().snapshot();
        assert_eq!(recovering.health, SpeechOutputHealth::Recovering);
        assert_eq!(recovering.channel_id, None);
        assert_eq!(
            recovering.last_error.as_ref().map(|err| err.kind),
            Some(SpeechOutputErrorKind::AudioQueueSaturated)
        );
        assert!(!shell.speech_retry_ready(1_000));
        assert!(shell.speech_retry_ready(1_000 + SPEECH_RETRY_BACKOFF_NS));

        shell.record_speech_failure(
            SpeechOutputError::from_voice_error(
                crate::audio::tts::select_voice("tr-TR", Some("gene")).unwrap_err(),
            ),
            None,
            2_000,
        );
        let failed = shell.speech_output.lock().snapshot();
        assert_eq!(failed.health, SpeechOutputHealth::FailedClosed);
        assert_eq!(
            failed.last_error.as_ref().map(|err| err.kind),
            Some(SpeechOutputErrorKind::VoiceLocaleMismatch)
        );
    }
}
