//! Custom canvas charts.
//!
//! M0 renders a synthetic latency series to prove the painting path. The real
//! chart consumes one-second aggregates from `benchlab-analysis` in M5.

use gpui::{Bounds, PathBuilder, Pixels, Window, canvas, div, point, prelude::*, px, rgba};

use crate::theme;

/// One plotted sample. Times are seconds since the start of the steady-state
/// phase; latency values are milliseconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChartPoint {
    pub t: f32,
    pub p50: f32,
    pub p95: f32,
    pub p99: f32,
}

/// Draw a latency chart with p50, p95, and p99 series.
pub fn latency_chart(points: Vec<ChartPoint>, duration_seconds: f32) -> impl IntoElement {
    let max_y = points.iter().map(|point| point.p99).fold(1.0_f32, f32::max) * 1.12;

    div().w_full().h_full().child(
        canvas(
            move |_, _, _| (),
            move |bounds, _, window, _| {
                paint_latency(bounds, &points, duration_seconds, max_y, window);
            },
        )
        .size_full(),
    )
}

fn paint_latency(
    bounds: Bounds<Pixels>,
    points: &[ChartPoint],
    duration_seconds: f32,
    max_y: f32,
    window: &mut Window,
) {
    if points.is_empty() || duration_seconds <= 0.0 {
        return;
    }

    let left = bounds.left() + px(6.);
    let right = bounds.right() - px(6.);
    let top = bounds.top() + px(10.);
    let bottom = bounds.bottom() - px(10.);
    let width = right - left;
    let height = bottom - top;

    let x_for = |t: f32| left + width * (t / duration_seconds).clamp(0.0, 1.0);
    let y_for = |value: f32| bottom - height * (value / max_y).clamp(0.0, 1.0);

    // Warm-up region (first minute) is context, not an objective sample.
    let warmup_end = x_for(60.0);
    let mut warmup = PathBuilder::fill();
    warmup.move_to(point(left, top));
    warmup.line_to(point(warmup_end, top));
    warmup.line_to(point(warmup_end, bottom));
    warmup.line_to(point(left, bottom));
    warmup.close();
    if let Ok(path) = warmup.build() {
        window.paint_path(path, rgba(0xFFFFFF08));
    }

    // Horizontal grid.
    for step in 0..=4 {
        let y = top + height * (step as f32 / 4.0);
        let mut line = PathBuilder::stroke(px(1.0));
        line.move_to(point(left, y));
        line.line_to(point(right, y));
        if let Ok(path) = line.build() {
            window.paint_path(path, rgba(0xFFFFFF0D));
        }
    }

    // Vertical grid.
    for step in 0..=4 {
        let x = left + width * (step as f32 / 4.0);
        let mut line = PathBuilder::stroke(px(1.0));
        line.move_to(point(x, top));
        line.line_to(point(x, bottom));
        if let Ok(path) = line.build() {
            window.paint_path(path, rgba(0xFFFFFF08));
        }
    }

    // 250 ms target line, intentionally dashed.
    let target_y = y_for(250.0);
    let mut target = PathBuilder::stroke(px(1.0)).dash_array(&[px(4.0), px(5.0)]);
    target.move_to(point(left, target_y));
    target.line_to(point(right, target_y));
    if let Ok(path) = target.build() {
        window.paint_path(path, rgba(0xD2992266));
    }

    paint_series(
        window,
        points,
        &x_for,
        &y_for,
        |point| point.p99,
        theme::chart_p99(),
        1.25,
    );
    paint_series(
        window,
        points,
        &x_for,
        &y_for,
        |point| point.p95,
        theme::chart_p95(),
        1.25,
    );
    paint_series(
        window,
        points,
        &x_for,
        &y_for,
        |point| point.p50,
        theme::chart_p50(),
        1.5,
    );
}

fn paint_series(
    window: &mut Window,
    points: &[ChartPoint],
    x_for: &impl Fn(f32) -> Pixels,
    y_for: &impl Fn(f32) -> Pixels,
    value: impl Fn(&ChartPoint) -> f32,
    color: gpui::Rgba,
    width: f32,
) {
    let mut builder = PathBuilder::stroke(px(width));
    for (index, sample) in points.iter().enumerate() {
        let x = x_for(sample.t);
        let y = y_for(value(sample));
        if index == 0 {
            builder.move_to(point(x, y));
        } else {
            builder.line_to(point(x, y));
        }
    }
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}
