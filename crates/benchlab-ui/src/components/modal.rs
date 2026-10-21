//! Modal and confirm-dialog primitives.
//!
//! The shell owns the backdrop and dismissal policy. Destructive dialogs pass
//! `dismissible = false` and require an explicit typed-name confirmation.

use gpui::{Div, FontWeight, IntoElement, SharedString, Stateful, div, prelude::*, px};

use crate::theme;

pub fn modal_panel(width: f32) -> Div {
    div()
        .flex()
        .flex_col()
        .w(px(width))
        .max_h(px(680.))
        .bg(theme::bg_surface())
        .border_1()
        .border_color(theme::border_strong())
        .rounded(px(8.))
        .overflow_hidden()
}

pub fn modal_header(title: impl Into<SharedString>, subtitle: Option<SharedString>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .px(px(20.))
        .pt(px(18.))
        .pb(px(14.))
        .border_b_1()
        .border_color(theme::border_subtle())
        .child(
            div()
                .text_size(px(15.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text_primary())
                .child(title.into()),
        )
        .when_some(subtitle, |this, subtitle| {
            this.child(
                div()
                    .text_size(px(11.))
                    .text_color(theme::text_muted())
                    .child(subtitle),
            )
        })
}

pub fn modal_body() -> Stateful<Div> {
    div()
        .id("modal-body")
        .flex()
        .flex_col()
        .gap_4()
        .p(px(20.))
        .max_h(px(540.))
        .overflow_y_scroll()
}

pub fn modal_footer() -> Div {
    div()
        .flex()
        .items_center()
        .justify_end()
        .gap_2()
        .px(px(20.))
        .py(px(14.))
        .border_t_1()
        .border_color(theme::border_subtle())
}

pub fn field_label(label: impl Into<SharedString>, hint: Option<SharedString>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text_secondary())
                .child(label.into()),
        )
        .when_some(hint, |this, hint| {
            this.child(
                div()
                    .text_size(px(10.))
                    .text_color(theme::text_muted())
                    .child(hint),
            )
        })
}

pub fn field(
    label: impl Into<SharedString>,
    hint: Option<SharedString>,
    control: impl IntoElement,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .w_full()
        .child(field_label(label, hint))
        .child(control)
}
