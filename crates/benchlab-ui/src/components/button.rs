//! BenchLab buttons. Controls are plain GPUI divs so views can attach
//! context-aware `cx.listener` handlers without losing type information.

use gpui::CursorStyle;
use gpui::{Div, FontWeight, SharedString, div, prelude::*, px, rgb};

use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonVariant {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonSize {
    Sm,
    Md,
}

impl ButtonSize {
    fn height(self) -> gpui::Pixels {
        match self {
            ButtonSize::Sm => px(28.),
            ButtonSize::Md => px(32.),
        }
    }
}

pub fn button(label: impl Into<SharedString>, variant: ButtonVariant, size: ButtonSize) -> Div {
    let label = label.into();
    let base = div()
        .flex()
        .items_center()
        .justify_center()
        .gap_2()
        .h(size.height())
        .px(px(12.))
        .rounded(px(6.))
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor(CursorStyle::PointingHand)
        .child(label);

    match variant {
        ButtonVariant::Primary => base
            .bg(theme::accent())
            .text_color(rgb(0xFFFFFF))
            .hover(|style| style.bg(theme::accent_hover())),
        ButtonVariant::Secondary => base
            .bg(theme::bg_surface())
            .border_1()
            .border_color(theme::border_default())
            .text_color(theme::text_secondary())
            .hover(|style| {
                style
                    .bg(theme::bg_raised())
                    .border_color(theme::border_strong())
            }),
        ButtonVariant::Ghost => base
            .bg(theme::bg_surface())
            .text_color(theme::text_secondary())
            .hover(|style| style.bg(theme::bg_raised())),
        ButtonVariant::Danger => base
            .bg(theme::danger())
            .text_color(rgb(0xFFFFFF))
            .hover(|style| style.bg(rgb(0xFF6B6B))),
    }
}

/// A compact, borderless button for row actions and icon-like controls.
pub fn row_action(label: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .h(px(24.))
        .px(px(7.))
        .rounded(px(4.))
        .text_size(px(11.))
        .text_color(theme::text_secondary())
        .cursor(CursorStyle::PointingHand)
        .hover(|style| {
            style
                .bg(theme::bg_raised())
                .text_color(theme::text_primary())
        })
        .child(label.into())
}

pub fn disabled(button: Div) -> Div {
    button.opacity(0.45).cursor(CursorStyle::Arrow)
}
