//! Switch and checkbox primitives. The caller owns the click handler and label
//! so the entire row can be a hit target.

use gpui::{Div, div, prelude::*, px};

use crate::theme;

pub fn switch(checked: bool) -> Div {
    div()
        .flex()
        .items_center()
        .w(px(30.))
        .h(px(17.))
        .p(px(2.))
        .rounded_full()
        .bg(if checked {
            theme::accent()
        } else {
            theme::border_strong()
        })
        .child(
            div()
                .size(px(13.))
                .rounded_full()
                .bg(if checked {
                    theme::text_primary()
                } else {
                    theme::text_muted()
                })
                .when(checked, |this| this.ml(px(13.))),
        )
}

pub fn checkbox(checked: bool) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .size(px(16.))
        .rounded(px(4.))
        .border_1()
        .border_color(if checked {
            theme::accent()
        } else {
            theme::border_strong()
        })
        .bg(if checked {
            theme::accent()
        } else {
            theme::bg_sunken()
        })
        .text_size(px(10.))
        .text_color(theme::text_primary())
        .when(checked, |this| this.child("✓"))
}
