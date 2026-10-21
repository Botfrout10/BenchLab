//! Toast cards. The shell owns the stack, lifetime, and maximum of three.

use gpui::{Div, FontWeight, Rgba, SharedString, div, prelude::*, px};

use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Danger,
}

impl ToastKind {
    pub fn color(self) -> Rgba {
        match self {
            ToastKind::Info => theme::info(),
            ToastKind::Success => theme::success(),
            ToastKind::Warning => theme::warning(),
            ToastKind::Danger => theme::danger(),
        }
    }
}

pub fn toast_card(title: impl Into<SharedString>, kind: ToastKind) -> Div {
    let color = kind.color();
    div()
        .flex()
        .items_start()
        .gap_2()
        .w(px(320.))
        .p(px(12.))
        .bg(theme::bg_raised())
        .border_1()
        .border_color(theme::with_alpha(color, 0.45))
        .rounded(px(8.))
        .child(div().mt(px(5.)).size(px(6.)).rounded_full().bg(color))
        .child(
            div()
                .flex_1()
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::text_primary())
                .child(title.into()),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(theme::text_muted())
                .child("×"),
        )
}

pub fn toast_success(title: impl Into<SharedString>) -> Div {
    toast_card(title, ToastKind::Success)
}

pub fn toast_danger(title: impl Into<SharedString>) -> Div {
    toast_card(title, ToastKind::Danger)
}
