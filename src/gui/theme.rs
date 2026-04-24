//! Hybrid Titan theme tokens for the echOS shell.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
    Auto,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellSurfaceRole {
    Wallpaper,
    HaloBar,
    Dock,
    Panel,
    Notification,
    WindowActive,
    WindowInactive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconFamily {
    System,
    Apps,
    Actions,
    Status,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconSize {
    Px20,
    Px24,
    Px32,
}

impl IconSize {
    pub const fn pixels(self) -> u32 {
        match self {
            IconSize::Px20 => 20,
            IconSize::Px24 => 24,
            IconSize::Px32 => 32,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellLayoutProfile {
    Desktop,
    Compact,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimationCurve {
    EaseOut,
    Spring,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpringPreset {
    pub stiffness: u16,
    pub damping: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnimationPreset {
    pub duration_ms: u16,
    pub curve: AnimationCurve,
    pub spring: SpringPreset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotionTokens {
    pub hover: AnimationPreset,
    pub press: AnimationPreset,
    pub focus: AnimationPreset,
    pub launch_minimize: AnimationPreset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceRole {
    Desktop,
    HaloBar,
    Dock,
    Window,
    WindowTitlebar,
    Sidebar,
    Field,
    Overlay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonRole {
    Primary,
    Secondary,
    Tertiary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowChromeVariant {
    Active,
    Inactive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Elevation {
    Resting,
    Floating,
    Focused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceTokens {
    pub desktop_top: u32,
    pub desktop_bottom: u32,
    pub wallpaper_glow: u32,
    pub halo_bar: u32,
    pub dock: u32,
    pub window: u32,
    pub window_titlebar: u32,
    pub window_titlebar_active: u32,
    pub sidebar: u32,
    pub field: u32,
    pub overlay: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextTokens {
    pub primary: u32,
    pub secondary: u32,
    pub tertiary: u32,
    pub on_accent: u32,
    pub on_dark: u32,
    pub disabled: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccentTokens {
    pub primary: u32,
    pub secondary: u32,
    pub success: u32,
    pub warning: u32,
    pub error: u32,
    pub glow: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BorderTokens {
    pub subtle: u32,
    pub strong: u32,
    pub focus: u32,
    pub chrome_glow: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShadowTokens {
    pub resting: u32,
    pub floating: u32,
    pub focused: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlurTokens {
    pub halo_bar: usize,
    pub dock: usize,
    pub window: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Radii {
    pub sm: usize,
    pub md: usize,
    pub lg: usize,
    pub xl: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spacing {
    pub xs: usize,
    pub sm: usize,
    pub md: usize,
    pub lg: usize,
    pub xl: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemeTokens {
    pub mode: ThemeMode,
    pub surfaces: SurfaceTokens,
    pub text: TextTokens,
    pub accent: AccentTokens,
    pub borders: BorderTokens,
    pub shadows: ShadowTokens,
    pub blur: BlurTokens,
    pub radii: Radii,
    pub spacing: Spacing,
    pub motion: MotionTokens,
}

pub struct Color;

impl Color {
    pub const TRANSPARENT: u32 = 0x00000000;

    pub const GRAPHITE_950: u32 = 0xFF091018;
    pub const GRAPHITE_900: u32 = 0xFF0D1520;
    pub const GRAPHITE_850: u32 = 0xFF121C28;
    pub const GRAPHITE_800: u32 = 0xFF172232;
    pub const GRAPHITE_700: u32 = 0xFF223246;
    pub const GRAPHITE_200: u32 = 0xFFD7E0EA;
    pub const GRAPHITE_100: u32 = 0xFFEAF0F6;

    pub const MIST_50: u32 = 0xFFF5F8FB;
    pub const MIST_100: u32 = 0xFFF0F5FA;
    pub const MIST_200: u32 = 0xFFE2EAF3;
    pub const MIST_300: u32 = 0xFFD3DDE8;
    pub const MIST_700: u32 = 0xFF536273;
    pub const MIST_800: u32 = 0xFF36475A;

    pub const ACCENT_AQUA: u32 = 0xFF26E6C6;
    pub const ACCENT_AZURE: u32 = 0xFF5AB3FF;
    pub const ACCENT_SUN: u32 = 0xFFFFB84D;
    pub const ACCENT_CORAL: u32 = 0xFFFF6B6B;
    pub const ACCENT_MINT: u32 = 0xFF4DDB95;

    pub const GLASS_DARK: u32 = 0xCC101925;
    pub const GLASS_LIGHT: u32 = 0xD8F7FAFD;
    pub const OVERLAY_DARK: u32 = 0xAA0A1118;
    pub const OVERLAY_LIGHT: u32 = 0x88F6F9FC;

    pub const DESKTOP_BG: u32 = Self::GRAPHITE_950;
    pub const WINDOW_BG: u32 = 0xF2192432;
    pub const TASKBAR_BG: u32 = 0xCC0F1722;
    pub const ACCENT_BLUE: u32 = Self::ACCENT_AZURE;
    pub const ACCENT_RED: u32 = Self::ACCENT_CORAL;
    pub const ACCENT_YELLOW: u32 = Self::ACCENT_SUN;
    pub const ACCENT_GREEN: u32 = Self::ACCENT_MINT;
    pub const TEXT_PRIMARY: u32 = 0xFFE9F0F7;
    pub const TEXT_SECONDARY: u32 = 0xFFA0AEBE;
    pub const TEXT_ON_DARK: u32 = 0xFFFFFFFF;
}

pub const DARK_THEME_TOKENS: ThemeTokens = ThemeTokens {
    mode: ThemeMode::Dark,
    surfaces: SurfaceTokens {
        desktop_top: 0xFF091018,
        desktop_bottom: 0xFF131F2D,
        wallpaper_glow: 0xFF123D4D,
        halo_bar: 0xCC0E1722,
        dock: 0xD6121B28,
        window: 0xF2182432,
        window_titlebar: 0xE61A2431,
        window_titlebar_active: 0xF11D2A3B,
        sidebar: 0xE6121B28,
        field: 0xE61A2430,
        overlay: 0xB20B1119,
    },
    text: TextTokens {
        primary: 0xFFE8EFF7,
        secondary: 0xFFA6B5C6,
        tertiary: 0xFF7A8B9D,
        on_accent: 0xFF081218,
        on_dark: 0xFFFFFFFF,
        disabled: 0xFF67788A,
    },
    accent: AccentTokens {
        primary: Color::ACCENT_AQUA,
        secondary: Color::ACCENT_AZURE,
        success: Color::ACCENT_MINT,
        warning: Color::ACCENT_SUN,
        error: Color::ACCENT_CORAL,
        glow: 0x6626E6C6,
    },
    borders: BorderTokens {
        subtle: 0xFF314355,
        strong: 0xFF4D6278,
        focus: 0xFF26E6C6,
        chrome_glow: 0xAA5AB3FF,
    },
    shadows: ShadowTokens {
        resting: 0x28000000,
        floating: 0x38000000,
        focused: 0x4D000000,
    },
    blur: BlurTokens {
        halo_bar: 10,
        dock: 14,
        window: 18,
    },
    radii: Radii {
        sm: 8,
        md: 12,
        lg: 14,
        xl: 20,
    },
    spacing: Spacing {
        xs: 4,
        sm: 8,
        md: 12,
        lg: 16,
        xl: 24,
    },
    motion: MotionTokens {
        hover: AnimationPreset {
            duration_ms: 120,
            curve: AnimationCurve::EaseOut,
            spring: SpringPreset {
                stiffness: 240,
                damping: 22,
            },
        },
        press: AnimationPreset {
            duration_ms: 70,
            curve: AnimationCurve::EaseOut,
            spring: SpringPreset {
                stiffness: 280,
                damping: 24,
            },
        },
        focus: AnimationPreset {
            duration_ms: 160,
            curve: AnimationCurve::Spring,
            spring: SpringPreset {
                stiffness: 220,
                damping: 20,
            },
        },
        launch_minimize: AnimationPreset {
            duration_ms: 220,
            curve: AnimationCurve::Spring,
            spring: SpringPreset {
                stiffness: 200,
                damping: 18,
            },
        },
    },
};

pub const LIGHT_THEME_TOKENS: ThemeTokens = ThemeTokens {
    mode: ThemeMode::Light,
    surfaces: SurfaceTokens {
        desktop_top: 0xFFF4F8FC,
        desktop_bottom: 0xFFE4ECF4,
        wallpaper_glow: 0xFFBDEEEA,
        halo_bar: 0xD8F7FAFD,
        dock: 0xE0F3F7FB,
        window: 0xF3FCFEFF,
        window_titlebar: 0xEEF4F8FB,
        window_titlebar_active: 0xFFF9FCFF,
        sidebar: 0xEAF0F5FA,
        field: 0xFFF1F6FA,
        overlay: 0x8CF6F9FC,
    },
    text: TextTokens {
        primary: 0xFF152233,
        secondary: 0xFF526376,
        tertiary: 0xFF71859A,
        on_accent: 0xFF041116,
        on_dark: 0xFFFFFFFF,
        disabled: 0xFF90A0AF,
    },
    accent: AccentTokens {
        primary: 0xFF16D4B5,
        secondary: 0xFF338DFF,
        success: 0xFF2FC97A,
        warning: 0xFFFFB13B,
        error: 0xFFF7605E,
        glow: 0x4416D4B5,
    },
    borders: BorderTokens {
        subtle: 0xFFD2DCE7,
        strong: 0xFFB2C0CF,
        focus: 0xFF16D4B5,
        chrome_glow: 0x66338DFF,
    },
    shadows: ShadowTokens {
        resting: 0x14000000,
        floating: 0x24000000,
        focused: 0x30000000,
    },
    blur: BlurTokens {
        halo_bar: 8,
        dock: 12,
        window: 14,
    },
    radii: Radii {
        sm: 8,
        md: 12,
        lg: 14,
        xl: 20,
    },
    spacing: Spacing {
        xs: 4,
        sm: 8,
        md: 12,
        lg: 16,
        xl: 24,
    },
    motion: MotionTokens {
        hover: AnimationPreset {
            duration_ms: 120,
            curve: AnimationCurve::EaseOut,
            spring: SpringPreset {
                stiffness: 240,
                damping: 22,
            },
        },
        press: AnimationPreset {
            duration_ms: 70,
            curve: AnimationCurve::EaseOut,
            spring: SpringPreset {
                stiffness: 280,
                damping: 24,
            },
        },
        focus: AnimationPreset {
            duration_ms: 160,
            curve: AnimationCurve::Spring,
            spring: SpringPreset {
                stiffness: 220,
                damping: 20,
            },
        },
        launch_minimize: AnimationPreset {
            duration_ms: 220,
            curve: AnimationCurve::Spring,
            spring: SpringPreset {
                stiffness: 200,
                damping: 18,
            },
        },
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorStruct {
    pub value: u32,
}

impl ColorStruct {
    pub const fn from_u32(value: u32) -> Self {
        Self { value }
    }

    pub const fn to_u32(self) -> u32 {
        self.value
    }

    pub const fn to_argb(self) -> u32 {
        self.value
    }
}

pub struct Theme;

impl Theme {
    pub const HALO_BAR_HEIGHT: usize = 60;
    pub const PULSE_DOCK_HEIGHT: usize = 68;
    pub const CORNER_RADIUS: usize = 14;
    pub const SHADOW_SPREAD: usize = 22;
    pub const WINDOW_TITLE_HEIGHT: usize = 34;
    pub const MIN_HIT_WIDTH: i32 = 44;
    pub const MIN_HIT_HEIGHT: i32 = 28;

    pub const fn default_mode() -> ThemeMode {
        ThemeMode::Dark
    }

    pub const fn resolve_mode(mode: ThemeMode, dark_preferred: bool) -> ThemeMode {
        match mode {
            ThemeMode::Dark => ThemeMode::Dark,
            ThemeMode::Light => ThemeMode::Light,
            ThemeMode::Auto => {
                if dark_preferred {
                    ThemeMode::Dark
                } else {
                    ThemeMode::Light
                }
            }
        }
    }

    pub const fn layout_profile(screen_width: u32) -> ShellLayoutProfile {
        if screen_width >= 1280 {
            ShellLayoutProfile::Desktop
        } else {
            ShellLayoutProfile::Compact
        }
    }

    pub const fn tokens(mode: ThemeMode) -> &'static ThemeTokens {
        match mode {
            ThemeMode::Dark => &DARK_THEME_TOKENS,
            ThemeMode::Light => &LIGHT_THEME_TOKENS,
            ThemeMode::Auto => &DARK_THEME_TOKENS,
        }
    }

    pub const fn resolved_tokens(mode: ThemeMode, dark_preferred: bool) -> &'static ThemeTokens {
        Self::tokens(Self::resolve_mode(mode, dark_preferred))
    }

    pub const fn surface(role: SurfaceRole, mode: ThemeMode, chrome: WindowChromeVariant) -> u32 {
        let tokens = Self::tokens(mode);
        match role {
            SurfaceRole::Desktop => tokens.surfaces.desktop_top,
            SurfaceRole::HaloBar => tokens.surfaces.halo_bar,
            SurfaceRole::Dock => tokens.surfaces.dock,
            SurfaceRole::Window => tokens.surfaces.window,
            SurfaceRole::WindowTitlebar => match chrome {
                WindowChromeVariant::Active => tokens.surfaces.window_titlebar_active,
                WindowChromeVariant::Inactive => tokens.surfaces.window_titlebar,
            },
            SurfaceRole::Sidebar => tokens.surfaces.sidebar,
            SurfaceRole::Field => tokens.surfaces.field,
            SurfaceRole::Overlay => tokens.surfaces.overlay,
        }
    }

    pub const fn shell_surface(
        role: ShellSurfaceRole,
        mode: ThemeMode,
        dark_preferred: bool,
    ) -> u32 {
        let resolved = Self::resolve_mode(mode, dark_preferred);
        match role {
            ShellSurfaceRole::Wallpaper => Self::surface(
                SurfaceRole::Desktop,
                resolved,
                WindowChromeVariant::Inactive,
            ),
            ShellSurfaceRole::HaloBar => Self::surface(
                SurfaceRole::HaloBar,
                resolved,
                WindowChromeVariant::Inactive,
            ),
            ShellSurfaceRole::Dock => {
                Self::surface(SurfaceRole::Dock, resolved, WindowChromeVariant::Inactive)
            }
            ShellSurfaceRole::Panel => Self::surface(
                SurfaceRole::Sidebar,
                resolved,
                WindowChromeVariant::Inactive,
            ),
            ShellSurfaceRole::Notification => Self::surface(
                SurfaceRole::Overlay,
                resolved,
                WindowChromeVariant::Inactive,
            ),
            ShellSurfaceRole::WindowActive => {
                Self::surface(SurfaceRole::Window, resolved, WindowChromeVariant::Active)
            }
            ShellSurfaceRole::WindowInactive => {
                Self::surface(SurfaceRole::Window, resolved, WindowChromeVariant::Inactive)
            }
        }
    }

    pub const fn animation_preset(
        mode: ThemeMode,
        dark_preferred: bool,
        role: ButtonRole,
    ) -> AnimationPreset {
        let motion = Self::resolved_tokens(mode, dark_preferred).motion;
        match role {
            ButtonRole::Primary => motion.focus,
            ButtonRole::Secondary => motion.hover,
            ButtonRole::Tertiary => motion.press,
        }
    }

    pub const fn shadow(level: Elevation, mode: ThemeMode) -> u32 {
        let shadows = Self::tokens(mode).shadows;
        match level {
            Elevation::Resting => shadows.resting,
            Elevation::Floating => shadows.floating,
            Elevation::Focused => shadows.focused,
        }
    }

    pub const fn button_fill(
        role: ButtonRole,
        mode: ThemeMode,
        pressed: bool,
        hovered: bool,
    ) -> u32 {
        let tokens = Self::tokens(mode);
        match role {
            ButtonRole::Primary => {
                if pressed {
                    Self::shade(tokens.accent.primary, -26)
                } else if hovered {
                    Self::shade(tokens.accent.primary, 18)
                } else {
                    tokens.accent.primary
                }
            }
            ButtonRole::Secondary => {
                let base = tokens.surfaces.field;
                if pressed {
                    Self::shade(base, -12)
                } else if hovered {
                    Self::shade(base, 10)
                } else {
                    base
                }
            }
            ButtonRole::Tertiary => {
                if pressed {
                    Self::shade(tokens.surfaces.overlay, -6)
                } else if hovered {
                    Self::shade(tokens.surfaces.overlay, 6)
                } else {
                    Color::TRANSPARENT
                }
            }
        }
    }

    pub const fn button_text(role: ButtonRole, mode: ThemeMode) -> u32 {
        let tokens = Self::tokens(mode);
        match role {
            ButtonRole::Primary => tokens.text.on_accent,
            ButtonRole::Secondary | ButtonRole::Tertiary => tokens.text.primary,
        }
    }

    pub fn luma(color: u32) -> u16 {
        let r = (color >> 16) & 0xFF;
        let g = (color >> 8) & 0xFF;
        let b = color & 0xFF;
        let weighted = r
            .saturating_mul(299)
            .saturating_add(g.saturating_mul(587))
            .saturating_add(b.saturating_mul(114));
        (weighted / 1000) as u16
    }

    pub fn luma_contrast(a: u32, b: u32) -> u16 {
        let a_luma = Self::luma(a);
        let b_luma = Self::luma(b);
        a_luma.max(b_luma) - a_luma.min(b_luma)
    }

    pub fn text_with_contrast(
        mode: ThemeMode,
        background: u32,
        preferred: u32,
        min_delta: u16,
    ) -> u32 {
        let tokens = Self::tokens(mode);
        let candidates = [
            preferred,
            tokens.text.secondary,
            tokens.text.primary,
            tokens.text.on_dark,
        ];
        for color in candidates {
            if Self::luma_contrast(color, background) >= min_delta {
                return color;
            }
        }
        tokens.text.primary
    }

    pub const fn get_accent() -> u32 {
        DARK_THEME_TOKENS.accent.primary
    }

    pub const fn shade(color: u32, delta: i16) -> u32 {
        let a = ((color >> 24) & 0xFF) as u32;
        let r = Self::shift_channel((color >> 16) & 0xFF, delta);
        let g = Self::shift_channel((color >> 8) & 0xFF, delta);
        let b = Self::shift_channel(color & 0xFF, delta);
        (a << 24) | (r << 16) | (g << 8) | b
    }

    const fn shift_channel(channel: u32, delta: i16) -> u32 {
        let value = channel as i32 + delta as i32;
        if value < 0 {
            0
        } else if value > 255 {
            255
        } else {
            value as u32
        }
    }

    pub const WINDOW_BG: ColorStruct = ColorStruct::from_u32(DARK_THEME_TOKENS.surfaces.window);
    pub const TITLEBAR_BG: ColorStruct =
        ColorStruct::from_u32(DARK_THEME_TOKENS.surfaces.window_titlebar);
    pub const TITLEBAR_ACTIVE: ColorStruct =
        ColorStruct::from_u32(DARK_THEME_TOKENS.surfaces.window_titlebar_active);
    pub const SIDEBAR_BG: ColorStruct = ColorStruct::from_u32(DARK_THEME_TOKENS.surfaces.sidebar);
    pub const TEXT_PRIMARY: ColorStruct = ColorStruct::from_u32(DARK_THEME_TOKENS.text.primary);
    pub const TEXT_SECONDARY: ColorStruct = ColorStruct::from_u32(DARK_THEME_TOKENS.text.secondary);
    pub const TEXT_TERTIARY: ColorStruct = ColorStruct::from_u32(DARK_THEME_TOKENS.text.tertiary);
    pub const TEXT_ACCENT: ColorStruct = ColorStruct::from_u32(DARK_THEME_TOKENS.accent.secondary);
    pub const TEXT_ON_ACCENT: ColorStruct = ColorStruct::from_u32(DARK_THEME_TOKENS.text.on_accent);
    pub const TEXT_DISABLED: ColorStruct = ColorStruct::from_u32(DARK_THEME_TOKENS.text.disabled);
    pub const BORDER: ColorStruct = ColorStruct::from_u32(DARK_THEME_TOKENS.borders.subtle);
    pub const BORDER_FOCUS: ColorStruct = ColorStruct::from_u32(DARK_THEME_TOKENS.borders.focus);
    pub const ACCENT_PRIMARY: ColorStruct = ColorStruct::from_u32(DARK_THEME_TOKENS.accent.primary);
    pub const ACCENT_SUCCESS: ColorStruct = ColorStruct::from_u32(DARK_THEME_TOKENS.accent.success);
    pub const ACCENT_WARNING: ColorStruct = ColorStruct::from_u32(DARK_THEME_TOKENS.accent.warning);
    pub const ACCENT_ERROR: ColorStruct = ColorStruct::from_u32(DARK_THEME_TOKENS.accent.error);
    pub const ERROR: ColorStruct = ColorStruct::from_u32(DARK_THEME_TOKENS.accent.error);
    pub const BUTTON_BG: ColorStruct = ColorStruct::from_u32(DARK_THEME_TOKENS.surfaces.field);
    pub const BUTTON_HOVER: ColorStruct =
        ColorStruct::from_u32(Self::shade(DARK_THEME_TOKENS.surfaces.field, 10));
    pub const BUTTON_TEXT: ColorStruct = ColorStruct::from_u32(DARK_THEME_TOKENS.text.primary);
    pub const INPUT_FOCUS: ColorStruct = ColorStruct::from_u32(DARK_THEME_TOKENS.borders.focus);
    pub const DESKTOP_BG: ColorStruct =
        ColorStruct::from_u32(DARK_THEME_TOKENS.surfaces.desktop_top);
    pub const SHADOW: ColorStruct = ColorStruct::from_u32(DARK_THEME_TOKENS.shadows.resting);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_spacing_and_radii_ladders_are_monotonic() {
        for tokens in [&DARK_THEME_TOKENS, &LIGHT_THEME_TOKENS] {
            assert!(tokens.spacing.xs < tokens.spacing.sm);
            assert!(tokens.spacing.sm < tokens.spacing.md);
            assert!(tokens.spacing.md < tokens.spacing.lg);
            assert!(tokens.spacing.lg < tokens.spacing.xl);
            assert!(tokens.radii.sm < tokens.radii.md);
            assert!(tokens.radii.md < tokens.radii.lg);
            assert!(tokens.radii.lg < tokens.radii.xl);
        }
    }

    #[test]
    fn theme_resolution_and_layout_profile_follow_screen_policy() {
        assert_eq!(Theme::resolve_mode(ThemeMode::Auto, true), ThemeMode::Dark);
        assert_eq!(
            Theme::resolve_mode(ThemeMode::Auto, false),
            ThemeMode::Light
        );
        assert_eq!(Theme::layout_profile(1279), ShellLayoutProfile::Compact);
        assert_eq!(Theme::layout_profile(1280), ShellLayoutProfile::Desktop);
    }
}
