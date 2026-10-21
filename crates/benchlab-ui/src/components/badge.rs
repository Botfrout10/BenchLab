//! Badges and pills for environment classes, verdicts, and methods.

use benchlab_core::EnvironmentKind;
use gpui::{Div, FontWeight, Rgba, SharedString, div, prelude::*, px};

use crate::theme;

pub fn badge(label: impl Into<SharedString>, color: Rgba) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1()
        .h(px(20.))
        .px(px(6.))
        .rounded(px(4.))
        .bg(theme::tint(color))
        .border_1()
        .border_color(theme::with_alpha(color, 0.3))
        .text_size(px(10.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(color)
        .child(div().size(px(5.)).rounded_full().bg(color))
        .child(label.into())
}

pub fn environment_badge(kind: EnvironmentKind) -> Div {
    badge(kind.label(), theme::environment_accent(kind))
}

pub fn neutral_badge(label: impl Into<SharedString>) -> Div {
    badge(label, theme::text_muted())
}

pub fn success_badge(label: impl Into<SharedString>) -> Div {
    badge(label, theme::success())
}

pub fn danger_badge(label: impl Into<SharedString>) -> Div {
    badge(label, theme::danger())
}

pub fn warning_badge(label: impl Into<SharedString>) -> Div {
    badge(label, theme::warning())
}
