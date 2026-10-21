//! Empty states: a short title and one sentence. Views add their own action
//! below when an action is appropriate.

use gpui::{Div, FontWeight, SharedString, div, prelude::*, px};

use crate::theme;

pub fn empty_state(title: impl Into<SharedString>, description: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .w_full()
        .min_h(px(180.))
        .p(px(24.))
        .child(
            div()
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text_primary())
                .child(title.into()),
        )
        .child(
            div()
                .max_w(px(420.))
                .text_size(px(11.))
                .text_color(theme::text_muted())
                .child(description.into()),
        )
}
