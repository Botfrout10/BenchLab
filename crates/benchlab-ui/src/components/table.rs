//! Table primitives. Small MVP lists render directly; run lists will switch to
//! `uniform_list` once they can grow beyond a few hundred rows.

use gpui::{AnyElement, Div, FontWeight, div, prelude::*, px};

use crate::theme;

#[derive(Clone, Copy, Debug)]
pub struct Column {
    pub label: &'static str,
    pub width: Option<f32>,
}

impl Column {
    pub const fn fixed(label: &'static str, width: f32) -> Self {
        Self {
            label,
            width: Some(width),
        }
    }

    pub const fn flex(label: &'static str) -> Self {
        Self { label, width: None }
    }
}

pub fn table_header(columns: &[Column]) -> Div {
    let mut header = div()
        .flex()
        .items_center()
        .w_full()
        .h(px(32.))
        .px(px(12.))
        .border_b_1()
        .border_color(theme::border_subtle())
        .text_size(px(9.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_muted());

    for (index, column) in columns.iter().enumerate() {
        header = header.child(cell_div(column, index == columns.len() - 1).child(column.label));
    }
    header
}

pub fn table_row(columns: &[Column], cells: Vec<AnyElement>, selected: bool) -> Div {
    let mut row = div()
        .flex()
        .items_center()
        .w_full()
        .min_h(px(40.))
        .px(px(12.))
        .border_b_1()
        .border_color(theme::border_subtle())
        .bg(if selected {
            theme::bg_raised()
        } else {
            theme::bg_surface()
        })
        .text_size(px(12.))
        .text_color(theme::text_secondary())
        .hover(|style| style.bg(theme::bg_raised()));

    for (index, cell) in cells.into_iter().enumerate() {
        let column = columns.get(index).copied().unwrap_or(Column::flex(""));
        row = row.child(cell_div(&column, index == columns.len() - 1).child(cell));
    }
    row
}

pub fn empty_table_row(columns: &[Column], message: impl Into<gpui::SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .w_full()
        .min_h(px(76.))
        .px(px(12.))
        .border_b_1()
        .border_color(theme::border_subtle())
        .text_size(px(11.))
        .text_color(theme::text_muted())
        .child(message.into())
        .when(columns.is_empty(), |this| this)
}

fn cell_div(column: &Column, last: bool) -> Div {
    let cell = div().flex().items_center().overflow_hidden();
    if last {
        cell.flex_1()
    } else if let Some(width) = column.width {
        cell.w(px(width)).flex_shrink_0()
    } else {
        cell.flex_1()
    }
}
