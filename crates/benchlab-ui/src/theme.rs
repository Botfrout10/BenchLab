//! BenchLab's visual tokens.
//!
//! Values mirror SPEC.md §8.2. Keep all surface, text, and data colors here so
//! views and charts cannot drift into different palettes.

use benchlab_core::{Accent, EnvironmentKind};
use gpui::{Rgba, rgb, rgba};

pub mod colors {
    pub const BG_APP: u32 = 0x0B0D10;
    pub const BG_SURFACE: u32 = 0x111418;
    pub const BG_RAISED: u32 = 0x171B21;
    pub const BG_SUNKEN: u32 = 0x080A0C;

    pub const BORDER_SUBTLE: u32 = 0x1F242B;
    pub const BORDER_DEFAULT: u32 = 0x2A313A;
    pub const BORDER_STRONG: u32 = 0x3A4350;

    pub const TEXT_PRIMARY: u32 = 0xE8EAED;
    pub const TEXT_SECONDARY: u32 = 0xA2AAB6;
    pub const TEXT_MUTED: u32 = 0x6C7684;

    pub const ACCENT: u32 = 0x5B8CFF;
    pub const ACCENT_HOVER: u32 = 0x6E9BFF;
    pub const SUCCESS: u32 = 0x3FB950;
    pub const WARNING: u32 = 0xD29922;
    pub const DANGER: u32 = 0xF05252;
    pub const INFO: u32 = 0x58A6FF;

    pub const CHART_P50: u32 = 0x7C8CF8;
    pub const CHART_P90: u32 = 0x56B4E9;
    pub const CHART_P95: u32 = 0xE69F00;
    pub const CHART_P99: u32 = 0xD55E00;
    pub const CHART_SUCCESS: u32 = 0x009E73;

    pub const PROJECT_BLUE: u32 = 0x5B8CFF;
    pub const PROJECT_TEAL: u32 = 0x2FB6A8;
    pub const PROJECT_VIOLET: u32 = 0xA78BFA;
    pub const PROJECT_AMBER: u32 = 0xE6A23C;
    pub const PROJECT_ROSE: u32 = 0xE06C75;
}

pub fn bg_app() -> Rgba {
    rgb(colors::BG_APP)
}

pub fn bg_surface() -> Rgba {
    rgb(colors::BG_SURFACE)
}

pub fn bg_raised() -> Rgba {
    rgb(colors::BG_RAISED)
}

pub fn bg_sunken() -> Rgba {
    rgb(colors::BG_SUNKEN)
}

pub fn bg_overlay() -> Rgba {
    rgba(0x05070ACF)
}

pub fn border_subtle() -> Rgba {
    rgb(colors::BORDER_SUBTLE)
}

pub fn border_default() -> Rgba {
    rgb(colors::BORDER_DEFAULT)
}

pub fn border_strong() -> Rgba {
    rgb(colors::BORDER_STRONG)
}

pub fn text_primary() -> Rgba {
    rgb(colors::TEXT_PRIMARY)
}

pub fn text_secondary() -> Rgba {
    rgb(colors::TEXT_SECONDARY)
}

pub fn text_muted() -> Rgba {
    rgb(colors::TEXT_MUTED)
}

pub fn accent() -> Rgba {
    rgb(colors::ACCENT)
}

pub fn accent_hover() -> Rgba {
    rgb(colors::ACCENT_HOVER)
}

pub fn success() -> Rgba {
    rgb(colors::SUCCESS)
}

pub fn warning() -> Rgba {
    rgb(colors::WARNING)
}

pub fn danger() -> Rgba {
    rgb(colors::DANGER)
}

pub fn info() -> Rgba {
    rgb(colors::INFO)
}

pub fn chart_p50() -> Rgba {
    rgb(colors::CHART_P50)
}

pub fn chart_p95() -> Rgba {
    rgb(colors::CHART_P95)
}

pub fn chart_p99() -> Rgba {
    rgb(colors::CHART_P99)
}

pub fn project_accent(accent: Accent) -> Rgba {
    match accent {
        Accent::Blue => rgb(colors::PROJECT_BLUE),
        Accent::Teal => rgb(colors::PROJECT_TEAL),
        Accent::Violet => rgb(colors::PROJECT_VIOLET),
        Accent::Amber => rgb(colors::PROJECT_AMBER),
        Accent::Rose => rgb(colors::PROJECT_ROSE),
    }
}

pub fn environment_accent(kind: EnvironmentKind) -> Rgba {
    match kind {
        EnvironmentKind::Local => rgb(colors::PROJECT_TEAL),
        EnvironmentKind::Staging => warning(),
        EnvironmentKind::Production => danger(),
    }
}

/// A semantic color with a specific alpha, for borders and fills.
pub fn with_alpha(color: Rgba, alpha: f32) -> Rgba {
    Rgba {
        a: color.a * alpha,
        ..color
    }
}

/// Subtle filled background for a semantic color.
pub fn tint(color: Rgba) -> Rgba {
    with_alpha(color, 0.12)
}
