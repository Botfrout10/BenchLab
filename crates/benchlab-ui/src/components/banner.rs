//! Inline banners for errors, warnings, and session-only storage notices.

use gpui::{Div, FontWeight, Rgba, SharedString, div, prelude::*, px};

use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BannerKind {
    Info,
    Success,
    Warning,
    Danger,
}

impl BannerKind {
    fn color(self) -> Rgba {
        match self {
            BannerKind::Info => theme::info(),
            BannerKind::Success => theme::success(),
            BannerKind::Warning => theme::warning(),
            BannerKind::Danger => theme::danger(),
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            BannerKind::Info => "i",
            BannerKind::Success => "✓",
            BannerKind::Warning => "!",
            BannerKind::Danger => "!",
        }
    }
}

pub fn banner(kind: BannerKind, message: impl Into<SharedString>) -> Div {
    let color = kind.color();
    div()
        .flex()
        .items_center()
        .gap_2()
        .w_full()
        .min_h(px(36.))
        .px(px(10.))
        .py(px(7.))
        .rounded(px(6.))
        .bg(theme::tint(color))
        .border_1()
        .border_color(theme::with_alpha(color, 0.35))
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .size(px(16.))
                .rounded_full()
                .bg(color)
                .text_size(px(9.))
                .font_weight(FontWeight::BOLD)
                .text_color(theme::bg_sunken())
                .child(kind.glyph()),
        )
        .child(
            div()
                .flex_1()
                .text_size(px(11.))
                .text_color(theme::text_secondary())
                .child(message.into()),
        )
}

pub fn inline_message(message: impl Into<SharedString>, danger: bool) -> Div {
    div()
        .w_full()
        .text_size(px(11.))
        .text_color(if danger {
            theme::danger()
        } else {
            theme::text_muted()
        })
        .child(message.into())
}
