//! # FAZ VII Personalization Engine
//!
//! Kişiselleştirme ve etkileşim altyapısını tek bir yerde toplar:
//! - Duvar kağıdından renk çıkarımı + sistem geneli anlık tema dağıtımı
//! - Otomatik karanlık/aydınlık mod + uygulama bazlı override
//! - Hibrit pencereleme (tiling/floating) + snap layouts
//! - Native/WASM/HTML widget motoru
//! - Sanal masaüstü profilleri + touchpad swipe geçişi
//! - Omni-search ve eklenti tabanlı arama
//! - Kontrol merkezi (hızlı ayarlar + medya)
//! - Bildirim merkezi (gruplama + DND + odak filtreleri)

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::cmp::{max, min};
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::gui::protocol::{
    AppId, NotificationEntry, NotificationLevel, Rect, WindowId, WorkspaceId,
};
use crate::gui::theme::ThemeMode;
use crate::gui::window_manager::{WindowError, WindowManager, MIN_CONTENT_HEIGHT, MIN_CONTENT_WIDTH};

// ============================================================================
// 7.1 Dynamic Theme ("Chameleon")
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppThemeOverride {
    FollowSystem,
    ForceDark,
    ForceLight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DynamicPalette {
    pub primary: u32,
    pub secondary: u32,
    pub tertiary: u32,
    pub surface: u32,
    pub on_primary: u32,
}

impl DynamicPalette {
    pub const fn default_dark() -> Self {
        Self {
            primary: 0xFF26E6C6,
            secondary: 0xFF5AB3FF,
            tertiary: 0xFFFFB84D,
            surface: 0xFF111A24,
            on_primary: 0xFF081218,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DayNightSchedule {
    pub light_start_hour: u8,
    pub dark_start_hour: u8,
}

impl Default for DayNightSchedule {
    fn default() -> Self {
        // 06:00-18:00 light, 18:00-06:00 dark
        Self {
            light_start_hour: 6,
            dark_start_hour: 18,
        }
    }
}

pub struct ChameleonThemeEngine {
    system_mode: ThemeMode,
    palette: DynamicPalette,
    schedule: DayNightSchedule,
    app_overrides: BTreeMap<AppId, AppThemeOverride>,
    subscribers: Vec<fn(DynamicPalette)>,
}

impl ChameleonThemeEngine {
    pub fn new() -> Self {
        Self {
            system_mode: ThemeMode::Dark,
            palette: DynamicPalette::default_dark(),
            schedule: DayNightSchedule::default(),
            app_overrides: BTreeMap::new(),
            subscribers: Vec::new(),
        }
    }

    pub fn palette(&self) -> DynamicPalette {
        self.palette
    }

    pub fn system_mode(&self) -> ThemeMode {
        self.system_mode
    }

    pub fn subscribe(&mut self, callback: fn(DynamicPalette)) {
        self.subscribers.push(callback);
    }

    pub fn set_app_override(&mut self, app_id: AppId, override_mode: AppThemeOverride) {
        self.app_overrides.insert(app_id, override_mode);
    }

    pub fn clear_app_override(&mut self, app_id: AppId) {
        self.app_overrides.remove(&app_id);
    }

    pub fn effective_mode_for_app(&self, app_id: AppId) -> ThemeMode {
        match self
            .app_overrides
            .get(&app_id)
            .copied()
            .unwrap_or(AppThemeOverride::FollowSystem)
        {
            AppThemeOverride::FollowSystem => self.system_mode,
            AppThemeOverride::ForceDark => ThemeMode::Dark,
            AppThemeOverride::ForceLight => ThemeMode::Light,
        }
    }

    pub fn set_day_night_schedule(&mut self, schedule: DayNightSchedule) {
        self.schedule = schedule;
    }

    pub fn apply_auto_mode_for_hour(&mut self, hour_24: u8) -> ThemeMode {
        let hour = hour_24 % 24;
        let light = self.schedule.light_start_hour % 24;
        let dark = self.schedule.dark_start_hour % 24;
        self.system_mode = if light < dark {
            if hour >= light && hour < dark {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            }
        } else if hour >= dark && hour < light {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        };
        self.system_mode
    }

    /// Duvar kağıdı örnek piksellerinden baskın renkleri çıkarır.
    ///
    /// Yaklaşım: 6x6x6 RGB histogram + doygunluk ağırlığı.
    /// Skor denklemi:
    /// `score(bin) = count(bin) * (1 + saturation(bin)/255)`
    pub fn derive_palette_from_wallpaper_samples(&mut self, argb_pixels: &[u32]) -> DynamicPalette {
        if argb_pixels.is_empty() {
            return self.palette;
        }

        let mut histogram = [0u32; 216];
        for &px in argb_pixels {
            let a = ((px >> 24) & 0xFF) as u8;
            if a < 16 {
                continue;
            }
            let r = ((px >> 16) & 0xFF) as usize;
            let g = ((px >> 8) & 0xFF) as usize;
            let b = (px & 0xFF) as usize;
            let rb = r / 43;
            let gb = g / 43;
            let bb = b / 43;
            let idx = rb * 36 + gb * 6 + bb;

            let max_c = max(r, max(g, b)) as u32;
            let min_c = min(r, min(g, b)) as u32;
            let sat = max_c.saturating_sub(min_c);
            let weight = 1 + sat / 32;
            histogram[idx] = histogram[idx].saturating_add(weight);
        }

        let mut top = [(0usize, 0u32); 3];
        for (idx, &count) in histogram.iter().enumerate() {
            if count == 0 {
                continue;
            }
            if count > top[0].1 {
                top[2] = top[1];
                top[1] = top[0];
                top[0] = (idx, count);
            } else if count > top[1].1 {
                top[2] = top[1];
                top[1] = (idx, count);
            } else if count > top[2].1 {
                top[2] = (idx, count);
            }
        }

        let primary = bin_to_argb(top[0].0);
        let secondary = if top[1].1 > 0 {
            bin_to_argb(top[1].0)
        } else {
            shade(primary, 24)
        };
        let tertiary = if top[2].1 > 0 {
            bin_to_argb(top[2].0)
        } else {
            blend(primary, secondary, 96)
        };

        let luma = perceived_luma(primary);
        let surface = if luma > 130 {
            shade(primary, -108)
        } else {
            shade(primary, -56)
        };
        let on_primary = if luma > 150 { 0xFF10151C } else { 0xFFF4F8FC };

        self.palette = DynamicPalette {
            primary,
            secondary,
            tertiary,
            surface,
            on_primary,
        };
        self.broadcast_palette();
        self.palette
    }

    pub fn derive_palette_from_gradient(&mut self, top: u32, bottom: u32) -> DynamicPalette {
        let primary = blend(top | 0xFF000000, bottom | 0xFF000000, 128);
        let secondary = shade(primary, 20);
        let tertiary = blend(secondary, 0xFFFFB84D, 64);
        let surface = shade(primary, -72);
        let on_primary = if perceived_luma(primary) > 150 {
            0xFF10151C
        } else {
            0xFFF5F9FC
        };

        self.palette = DynamicPalette {
            primary,
            secondary,
            tertiary,
            surface,
            on_primary,
        };
        self.broadcast_palette();
        self.palette
    }

    fn broadcast_palette(&self) {
        for callback in &self.subscribers {
            callback(self.palette);
        }
    }
}

impl Default for ChameleonThemeEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 7.2 Hybrid Windowing + Snap Layouts
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowLayoutMode {
    Floating,
    Tiling,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapLayout {
    LeftHalf,
    RightHalf,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Maximize,
}

pub struct HybridWindowOrchestrator {
    mode: WindowLayoutMode,
}

impl HybridWindowOrchestrator {
    pub fn new() -> Self {
        Self {
            mode: WindowLayoutMode::Floating,
        }
    }

    pub fn mode(&self) -> WindowLayoutMode {
        self.mode
    }

    pub fn toggle_mode(&mut self) -> WindowLayoutMode {
        self.mode = match self.mode {
            WindowLayoutMode::Floating => WindowLayoutMode::Tiling,
            WindowLayoutMode::Tiling => WindowLayoutMode::Floating,
        };
        self.mode
    }

    /// Grid-temelli tiling: `c = ceil(sqrt(n))`, `r = ceil(n/c)`.
    pub fn apply_tiling(&self, wm: &mut WindowManager, work_area: Rect) -> Result<u32, WindowError> {
        let windows = wm.ordered_windows();
        let visible: Vec<_> = windows.into_iter().filter(|w| w.visible).collect();
        if visible.is_empty() {
            return Ok(0);
        }

        let n = visible.len() as u32;
        let mut cols = 1u32;
        while cols.saturating_mul(cols) < n {
            cols = cols.saturating_add(1);
        }
        let rows = (n + cols - 1) / cols;
        let tile_w = max(work_area.width / cols, MIN_CONTENT_WIDTH);
        let tile_h = max(work_area.height / rows, MIN_CONTENT_HEIGHT);

        let mut touched = 0u32;
        for (idx, win) in visible.iter().enumerate() {
            let i = idx as u32;
            let col = i % cols;
            let row = i / cols;
            let x = work_area.x + (col * tile_w) as i32;
            let y = work_area.y + (row * tile_h) as i32;
            wm.set_window_frame(win.id, x, y, tile_w, tile_h)?;
            touched = touched.saturating_add(1);
        }
        Ok(touched)
    }

    pub fn snap_window(
        &self,
        wm: &mut WindowManager,
        window_id: WindowId,
        work_area: Rect,
        layout: SnapLayout,
    ) -> Result<(), WindowError> {
        let half_w = max(work_area.width / 2, MIN_CONTENT_WIDTH);
        let half_h = max(work_area.height / 2, MIN_CONTENT_HEIGHT);
        let (x, y, w, h) = match layout {
            SnapLayout::LeftHalf => (work_area.x, work_area.y, half_w, work_area.height),
            SnapLayout::RightHalf => (
                work_area.x + half_w as i32,
                work_area.y,
                half_w,
                work_area.height,
            ),
            SnapLayout::TopLeft => (work_area.x, work_area.y, half_w, half_h),
            SnapLayout::TopRight => (
                work_area.x + half_w as i32,
                work_area.y,
                half_w,
                half_h,
            ),
            SnapLayout::BottomLeft => (
                work_area.x,
                work_area.y + half_h as i32,
                half_w,
                half_h,
            ),
            SnapLayout::BottomRight => (
                work_area.x + half_w as i32,
                work_area.y + half_h as i32,
                half_w,
                half_h,
            ),
            SnapLayout::Maximize => {
                wm.toggle_maximize(window_id, work_area)?;
                return Ok(());
            }
        };
        wm.set_window_frame(window_id, x, y, w, h)?;
        Ok(())
    }
}

impl Default for HybridWindowOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 7.2 Widget Engine
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetRuntime {
    NativeRust,
    Wasm,
    Html,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WidgetManifest {
    pub id: String,
    pub title: String,
    pub runtime: WidgetRuntime,
    pub entry: String,
    pub refresh_ms: u32,
    pub permissions: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WidgetPlacement {
    pub widget_id: String,
    pub rect: Rect,
    pub workspace_id: WorkspaceId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WidgetUpdateEvent {
    pub widget_id: String,
    pub tick: u64,
    pub reason: String,
}

pub struct WidgetEngine {
    installed: BTreeMap<String, WidgetManifest>,
    placements: BTreeMap<WorkspaceId, Vec<WidgetPlacement>>,
    last_tick_ms: BTreeMap<String, u64>,
    tick_counter: AtomicU64,
}

impl WidgetEngine {
    pub fn new() -> Self {
        Self {
            installed: BTreeMap::new(),
            placements: BTreeMap::new(),
            last_tick_ms: BTreeMap::new(),
            tick_counter: AtomicU64::new(0),
        }
    }

    pub fn install_widget(&mut self, manifest: WidgetManifest) {
        self.last_tick_ms.insert(manifest.id.clone(), 0);
        self.installed.insert(manifest.id.clone(), manifest);
    }

    pub fn parse_and_install_manifest(&mut self, manifest_text: &str) -> Result<(), String> {
        let mut id = String::new();
        let mut title = String::new();
        let mut entry = String::new();
        let mut refresh_ms = 1000u32;
        let mut runtime = WidgetRuntime::NativeRust;
        let mut permissions = Vec::new();

        for line in manifest_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let (k, v) = match trimmed.split_once('=') {
                Some(pair) => pair,
                None => continue,
            };
            let key = k.trim();
            let value = v.trim().trim_matches('"');
            match key {
                "id" => id = value.to_string(),
                "title" => title = value.to_string(),
                "entry" => entry = value.to_string(),
                "refresh_ms" => {
                    refresh_ms = value.parse::<u32>().unwrap_or(1000);
                }
                "runtime" => {
                    runtime = match value {
                        "wasm" => WidgetRuntime::Wasm,
                        "html" => WidgetRuntime::Html,
                        _ => WidgetRuntime::NativeRust,
                    };
                }
                "permissions" => {
                    permissions = value
                        .split(',')
                        .map(|p| p.trim().to_string())
                        .filter(|p| !p.is_empty())
                        .collect();
                }
                _ => {}
            }
        }

        if id.is_empty() || title.is_empty() || entry.is_empty() {
            return Err(String::from("widget manifest missing id/title/entry"));
        }

        self.install_widget(WidgetManifest {
            id,
            title,
            runtime,
            entry,
            refresh_ms,
            permissions,
        });
        Ok(())
    }

    pub fn place_widget(&mut self, widget_id: &str, workspace_id: WorkspaceId, rect: Rect) -> Result<(), String> {
        if !self.installed.contains_key(widget_id) {
            return Err(format!("widget not installed: {}", widget_id));
        }
        let placements = self.placements.entry(workspace_id).or_insert_with(Vec::new);
        if let Some(existing) = placements.iter_mut().find(|p| p.widget_id == widget_id) {
            existing.rect = rect;
            return Ok(());
        }
        placements.push(WidgetPlacement {
            widget_id: widget_id.to_string(),
            rect,
            workspace_id,
        });
        Ok(())
    }

    pub fn widgets_for_workspace(&self, workspace_id: WorkspaceId) -> Vec<WidgetPlacement> {
        self.placements
            .get(&workspace_id)
            .cloned()
            .unwrap_or_else(Vec::new)
    }

    pub fn tick(&mut self, now_ms: u64) -> Vec<WidgetUpdateEvent> {
        let mut events = Vec::new();
        for (id, manifest) in self.installed.iter() {
            let last = self.last_tick_ms.get(id).copied().unwrap_or(0);
            if now_ms.saturating_sub(last) < manifest.refresh_ms as u64 {
                continue;
            }
            self.last_tick_ms.insert(id.clone(), now_ms);
            let tick = self.tick_counter.fetch_add(1, Ordering::Relaxed).saturating_add(1);
            events.push(WidgetUpdateEvent {
                widget_id: id.clone(),
                tick,
                reason: String::from("refresh"),
            });
        }
        events
    }
}

impl Default for WidgetEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 7.2 Virtual Desktops (profiles + swipe)
// ============================================================================

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopProfile {
    pub wallpaper_id: u32,
    pub icon_pack: String,
}

pub struct VirtualDesktopManager {
    max_desktops: WorkspaceId,
    active: WorkspaceId,
    profiles: BTreeMap<WorkspaceId, DesktopProfile>,
    swipe_threshold_px: i32,
}

impl VirtualDesktopManager {
    pub fn new(max_desktops: WorkspaceId) -> Self {
        let max_desktops = max(1, max_desktops);
        let mut profiles = BTreeMap::new();
        for id in 0..max_desktops {
            profiles.insert(
                id,
                DesktopProfile {
                    wallpaper_id: id as u32,
                    icon_pack: format!("default-{}", id),
                },
            );
        }
        Self {
            max_desktops,
            active: 0,
            profiles,
            swipe_threshold_px: 96,
        }
    }

    pub fn active(&self) -> WorkspaceId {
        self.active
    }

    pub fn profile(&self, workspace_id: WorkspaceId) -> Option<&DesktopProfile> {
        self.profiles.get(&workspace_id)
    }

    pub fn set_profile(&mut self, workspace_id: WorkspaceId, profile: DesktopProfile) -> Result<(), String> {
        if workspace_id >= self.max_desktops {
            return Err(String::from("workspace out of range"));
        }
        self.profiles.insert(workspace_id, profile);
        Ok(())
    }

    pub fn switch_to(&mut self, workspace_id: WorkspaceId) -> Result<WorkspaceId, String> {
        if workspace_id >= self.max_desktops {
            return Err(String::from("workspace out of range"));
        }
        self.active = workspace_id;
        Ok(self.active)
    }

    pub fn swipe_switch(&mut self, delta_x: i32, delta_y: i32) -> Option<WorkspaceId> {
        if delta_x.abs() < self.swipe_threshold_px || delta_x.abs() <= delta_y.abs() {
            return None;
        }

        let next = if delta_x < 0 {
            self.active.saturating_add(1).min(self.max_desktops.saturating_sub(1))
        } else {
            self.active.saturating_sub(1)
        };

        if next == self.active {
            return None;
        }
        self.active = next;
        Some(self.active)
    }
}

// ============================================================================
// 7.3 Omni-Search + plugin architecture
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OmniSource {
    File,
    App,
    Web,
    Calculator,
    UnitConverter,
    Plugin,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchResult {
    pub source: OmniSource,
    pub title: String,
    pub subtitle: String,
    pub score: i32,
}

pub type SearchPlugin = fn(&str) -> Vec<SearchResult>;

pub struct OmniSearchEngine {
    file_index: Vec<String>,
    app_index: Vec<String>,
    plugins: BTreeMap<String, SearchPlugin>,
}

impl OmniSearchEngine {
    pub fn new() -> Self {
        Self {
            file_index: Vec::new(),
            app_index: Vec::new(),
            plugins: BTreeMap::new(),
        }
    }

    pub fn index_files(&mut self, files: Vec<String>) {
        self.file_index = files;
    }

    pub fn index_apps(&mut self, apps: Vec<String>) {
        self.app_index = apps;
    }

    pub fn register_plugin(&mut self, name: &str, plugin: SearchPlugin) {
        self.plugins.insert(name.to_string(), plugin);
    }

    pub fn query(&self, raw_query: &str) -> Vec<SearchResult> {
        let query = raw_query.trim();
        if query.is_empty() {
            return Vec::new();
        }
        let ql = query.to_lowercase();
        let mut out = Vec::new();

        for path in &self.file_index {
            let pl = path.to_lowercase();
            if pl.contains(&ql) {
                out.push(SearchResult {
                    source: OmniSource::File,
                    title: path.clone(),
                    subtitle: String::from("File"),
                    score: score_substring(&pl, &ql, 120),
                });
            }
        }

        for app in &self.app_index {
            let al = app.to_lowercase();
            if al.contains(&ql) {
                out.push(SearchResult {
                    source: OmniSource::App,
                    title: app.clone(),
                    subtitle: String::from("Application"),
                    score: score_substring(&al, &ql, 140),
                });
            }
        }

        if let Some(value) = eval_expression(query) {
            out.push(SearchResult {
                source: OmniSource::Calculator,
                title: format!("{} = {}", query, value),
                subtitle: String::from("Calculator"),
                score: 220,
            });
        }

        if let Some(converted) = convert_units(query) {
            out.push(SearchResult {
                source: OmniSource::UnitConverter,
                title: converted,
                subtitle: String::from("Unit conversion"),
                score: 210,
            });
        }

        out.push(SearchResult {
            source: OmniSource::Web,
            title: format!("Search web for \"{}\"", query),
            subtitle: format!("https://duckduckgo.com/?q={}", encode_query(query)),
            score: 80,
        });

        for plugin in self.plugins.values() {
            let mut results = plugin(query);
            for result in results.iter_mut() {
                result.source = OmniSource::Plugin;
                result.score = result.score.saturating_add(90);
            }
            out.append(&mut results);
        }

        out.sort_by(|a, b| b.score.cmp(&a.score));
        out.truncate(32);
        out
    }
}

impl Default for OmniSearchEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 7.3 Control Center
// ============================================================================

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaState {
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub position_ms: u32,
}

impl Default for MediaState {
    fn default() -> Self {
        Self {
            title: String::new(),
            artist: String::new(),
            playing: false,
            position_ms: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickSettingsState {
    pub wifi_enabled: bool,
    pub bluetooth_enabled: bool,
    pub volume_percent: u8,
    pub brightness_percent: u8,
}

impl Default for QuickSettingsState {
    fn default() -> Self {
        Self {
            wifi_enabled: true,
            bluetooth_enabled: false,
            volume_percent: 40,
            brightness_percent: 70,
        }
    }
}

pub struct ControlCenterState {
    pub quick: QuickSettingsState,
    pub media: MediaState,
}

impl ControlCenterState {
    pub fn new() -> Self {
        Self {
            quick: QuickSettingsState::default(),
            media: MediaState::default(),
        }
    }

    pub fn toggle_wifi(&mut self) -> bool {
        self.quick.wifi_enabled = !self.quick.wifi_enabled;
        self.quick.wifi_enabled
    }

    pub fn toggle_bluetooth(&mut self) -> bool {
        self.quick.bluetooth_enabled = !self.quick.bluetooth_enabled;
        self.quick.bluetooth_enabled
    }

    pub fn set_volume(&mut self, percent: u8) {
        self.quick.volume_percent = min(percent, 100);
    }

    pub fn set_brightness(&mut self, percent: u8) {
        self.quick.brightness_percent = min(percent, 100);
    }

    pub fn update_media(&mut self, title: &str, artist: &str, playing: bool, position_ms: u32) {
        self.media.title = title.to_string();
        self.media.artist = artist.to_string();
        self.media.playing = playing;
        self.media.position_ms = position_ms;
    }
}

impl Default for ControlCenterState {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 7.3 Notification Center (grouping + DND + focus filters)
// ============================================================================

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FocusFilter {
    pub allowed_apps: BTreeSet<AppId>,
    pub min_level: NotificationLevel,
}

impl Default for FocusFilter {
    fn default() -> Self {
        Self {
            allowed_apps: BTreeSet::new(),
            min_level: NotificationLevel::Info,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationGroup {
    pub key: String,
    pub app_id: AppId,
    pub title: String,
    pub entries: Vec<NotificationEntry>,
}

pub struct NotificationCenter {
    groups: BTreeMap<String, NotificationGroup>,
    do_not_disturb: bool,
    focus_filter: FocusFilter,
}

impl NotificationCenter {
    pub fn new() -> Self {
        Self {
            groups: BTreeMap::new(),
            do_not_disturb: false,
            focus_filter: FocusFilter::default(),
        }
    }

    pub fn set_do_not_disturb(&mut self, enabled: bool) {
        self.do_not_disturb = enabled;
    }

    pub fn configure_focus_filter(&mut self, filter: FocusFilter) {
        self.focus_filter = filter;
    }

    pub fn push(&mut self, entry: NotificationEntry) -> bool {
        if !self.accepts(&entry) {
            return false;
        }
        let key = format!("{}:{}", entry.app_id, entry.title);
        let group = self.groups.entry(key.clone()).or_insert_with(|| NotificationGroup {
            key: key.clone(),
            app_id: entry.app_id,
            title: entry.title.clone(),
            entries: Vec::new(),
        });
        group.entries.push(entry);
        true
    }

    pub fn groups(&self) -> Vec<NotificationGroup> {
        self.groups.values().cloned().collect()
    }

    pub fn clear_group(&mut self, key: &str) {
        self.groups.remove(key);
    }

    pub fn clear_all(&mut self) {
        self.groups.clear();
    }

    fn accepts(&self, entry: &NotificationEntry) -> bool {
        if self.do_not_disturb && entry.level != NotificationLevel::Error {
            return false;
        }

        let level_ok = notification_level_rank(entry.level)
            >= notification_level_rank(self.focus_filter.min_level);
        if !level_ok {
            return false;
        }
        if self.focus_filter.allowed_apps.is_empty() {
            return true;
        }
        self.focus_filter.allowed_apps.contains(&entry.app_id)
    }
}

impl Default for NotificationCenter {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Global accessors
// ============================================================================

lazy_static::lazy_static! {
    static ref CHAMELEON_THEME: Mutex<ChameleonThemeEngine> = Mutex::new(ChameleonThemeEngine::new());
    static ref HYBRID_WM: Mutex<HybridWindowOrchestrator> = Mutex::new(HybridWindowOrchestrator::new());
    static ref WIDGET_ENGINE: Mutex<WidgetEngine> = Mutex::new(WidgetEngine::new());
    static ref VIRTUAL_DESKTOPS: Mutex<VirtualDesktopManager> = Mutex::new(VirtualDesktopManager::new(8));
    static ref OMNI_SEARCH: Mutex<OmniSearchEngine> = Mutex::new(OmniSearchEngine::new());
    static ref CONTROL_CENTER: Mutex<ControlCenterState> = Mutex::new(ControlCenterState::new());
    static ref NOTIFICATION_CENTER: Mutex<NotificationCenter> = Mutex::new(NotificationCenter::new());
}

pub fn chameleon_theme() -> &'static Mutex<ChameleonThemeEngine> {
    &CHAMELEON_THEME
}

pub fn hybrid_windowing() -> &'static Mutex<HybridWindowOrchestrator> {
    &HYBRID_WM
}

pub fn widget_engine() -> &'static Mutex<WidgetEngine> {
    &WIDGET_ENGINE
}

pub fn virtual_desktops() -> &'static Mutex<VirtualDesktopManager> {
    &VIRTUAL_DESKTOPS
}

pub fn omni_search() -> &'static Mutex<OmniSearchEngine> {
    &OMNI_SEARCH
}

pub fn control_center() -> &'static Mutex<ControlCenterState> {
    &CONTROL_CENTER
}

pub fn notification_center() -> &'static Mutex<NotificationCenter> {
    &NOTIFICATION_CENTER
}

// ============================================================================
// Helpers
// ============================================================================

fn notification_level_rank(level: NotificationLevel) -> u8 {
    match level {
        NotificationLevel::Info => 0,
        NotificationLevel::Success => 1,
        NotificationLevel::Warning => 2,
        NotificationLevel::Error => 3,
    }
}

fn score_substring(haystack: &str, needle: &str, base: i32) -> i32 {
    if let Some(pos) = haystack.find(needle) {
        base.saturating_sub(pos as i32)
    } else {
        0
    }
}

fn encode_query(q: &str) -> String {
    q.chars()
        .map(|c| if c == ' ' { '+' } else { c })
        .collect()
}

fn eval_expression(expr: &str) -> Option<i64> {
    // Sabit biçim parser: "<a> <op> <b>"
    let mut parts = expr.split_whitespace();
    let a = parts.next()?.parse::<i64>().ok()?;
    let op = parts.next()?;
    let b = parts.next()?.parse::<i64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    match op {
        "+" => Some(a.saturating_add(b)),
        "-" => Some(a.saturating_sub(b)),
        "*" => Some(a.saturating_mul(b)),
        "/" => {
            if b == 0 {
                None
            } else {
                Some(a / b)
            }
        }
        _ => None,
    }
}

fn convert_units(expr: &str) -> Option<String> {
    // Format: "<value> <unit_from> to <unit_to>"
    let mut parts = expr.split_whitespace();
    let value = parts.next()?.parse::<f64>().ok()?;
    let from = parts.next()?.to_lowercase();
    let to_kw = parts.next()?.to_lowercase();
    let to = parts.next()?.to_lowercase();
    if to_kw != "to" {
        return None;
    }

    let out = match (from.as_str(), to.as_str()) {
        ("km", "m") => value * 1000.0,
        ("m", "km") => value / 1000.0,
        ("gb", "mb") => value * 1024.0,
        ("mb", "gb") => value / 1024.0,
        ("c", "f") => (value * 9.0 / 5.0) + 32.0,
        ("f", "c") => (value - 32.0) * 5.0 / 9.0,
        _ => return None,
    };
    Some(format!("{:.3} {} = {:.3} {}", value, from, out, to))
}

fn bin_to_argb(bin: usize) -> u32 {
    let rb = (bin / 36) % 6;
    let gb = (bin / 6) % 6;
    let bb = bin % 6;
    let r = (rb * 43 + 21) as u32;
    let g = (gb * 43 + 21) as u32;
    let b = (bb * 43 + 21) as u32;
    0xFF000000 | (r << 16) | (g << 8) | b
}

fn perceived_luma(argb: u32) -> u32 {
    let r = (argb >> 16) & 0xFF;
    let g = (argb >> 8) & 0xFF;
    let b = argb & 0xFF;
    (r * 212 + g * 715 + b * 72) / 1000
}

fn shade(argb: u32, delta: i16) -> u32 {
    let a = (argb >> 24) & 0xFF;
    let r = shift_channel((argb >> 16) & 0xFF, delta);
    let g = shift_channel((argb >> 8) & 0xFF, delta);
    let b = shift_channel(argb & 0xFF, delta);
    (a << 24) | (r << 16) | (g << 8) | b
}

fn shift_channel(channel: u32, delta: i16) -> u32 {
    let value = channel as i32 + delta as i32;
    if value < 0 {
        0
    } else if value > 255 {
        255
    } else {
        value as u32
    }
}

fn blend(a: u32, b: u32, ratio_255: u32) -> u32 {
    let t = min(ratio_255, 255);
    let inv = 255 - t;

    let ar = (a >> 16) & 0xFF;
    let ag = (a >> 8) & 0xFF;
    let ab = a & 0xFF;

    let br = (b >> 16) & 0xFF;
    let bg = (b >> 8) & 0xFF;
    let bb = b & 0xFF;

    let r = (ar * inv + br * t) / 255;
    let g = (ag * inv + bg * t) / 255;
    let b = (ab * inv + bb * t) / 255;

    0xFF000000 | (r << 16) | (g << 8) | b
}
