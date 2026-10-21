//! Native text input built on GPUI's low-level text element.
//!
//! The component supports both single-line fields and explicit-newline
//! multiline editors (used by the scenario body panel). It keeps native
//! selection, clipboard, IME, and grapheme-aware cursor movement.

use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, FocusHandle, Focusable, GlobalElementId, Hsla, LayoutId,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point,
    ShapedLine, SharedString, Style, TextRun, UTF16Selection, UnderlineStyle, Window, actions, div,
    fill, point, prelude::*, px, relative, rgba, size,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::theme;

pub type ChangeCallback = Box<dyn Fn(&str, &mut Window, &mut App)>;

actions!(
    text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectAll,
        Home,
        End,
        Enter,
        Paste,
        Cut,
        Copy,
    ]
);

#[derive(Clone, Default)]
struct TextLayout {
    lines: Vec<ShapedLine>,
    line_starts: Vec<usize>,
    line_height: Pixels,
}

pub struct TextInput {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<TextLayout>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
    masked: bool,
    multiline: bool,
    rows: usize,
    error: Option<SharedString>,
    on_change: Option<ChangeCallback>,
}

impl TextInput {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: "".into(),
            placeholder: placeholder.into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            is_selecting: false,
            masked: false,
            multiline: false,
            rows: 1,
            error: None,
            on_change: None,
        }
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn masked(&self) -> bool {
        self.masked
    }

    pub fn is_multiline(&self) -> bool {
        self.multiline
    }

    pub fn set_masked(&mut self, masked: bool, cx: &mut Context<Self>) {
        if self.masked != masked {
            self.masked = masked;
            self.last_layout = None;
            cx.notify();
        }
    }

    pub fn set_multiline(&mut self, multiline: bool, rows: usize, cx: &mut Context<Self>) {
        let rows = rows.max(1);
        if self.multiline != multiline || self.rows != rows {
            self.multiline = multiline;
            self.rows = rows;
            self.last_layout = None;
            cx.notify();
        }
    }

    pub fn set_rows(&mut self, rows: usize, cx: &mut Context<Self>) {
        let rows = rows.max(1);
        if self.rows != rows {
            self.rows = rows;
            self.last_layout = None;
            cx.notify();
        }
    }

    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    pub fn set_error(&mut self, error: Option<SharedString>, cx: &mut Context<Self>) {
        if self.error != error {
            self.error = error;
            cx.notify();
        }
    }

    pub fn set_on_change(&mut self, callback: Option<ChangeCallback>) {
        self.on_change = callback;
    }

    pub fn set_content(&mut self, content: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.content = content.into();
        self.selected_range = self.content.len()..self.content.len();
        self.selection_reversed = false;
        self.marked_range = None;
        self.last_layout = None;
        self.last_bounds = None;
        cx.notify();
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx)
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx)
        }
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical_move(-1, false, cx);
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical_move(1, false, cx);
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical_move(-1, true, cx);
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical_move(1, true, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx)
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            let starts = line_starts(&self.content);
            let line = line_for_offset(&starts, self.cursor_offset());
            self.move_to(starts.get(line).copied().unwrap_or(0), cx);
        } else {
            self.move_to(0, cx);
        }
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            let starts = line_starts(&self.content);
            let line = line_for_offset(&starts, self.cursor_offset());
            self.move_to(line_end(&self.content, &starts, line), cx);
        } else {
            self.move_to(self.content.len(), cx);
        }
    }

    fn insert_newline(&mut self, _: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            self.replace_text_in_range(None, "\n", window, cx);
        }
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        self.is_selecting = true;

        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), cx)
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let text = text.replace("\r\n", "\n").replace('\r', "\n");
            let text = if self.multiline {
                text
            } else {
                text.replace(['\n', '\r'], " ")
            };
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx)
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        cx.notify()
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        let (Some(bounds), Some(layout)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        let line_height = layout.line_height.max(px(1.));
        let local_y = position.y - bounds.top();
        let mut line = (local_y / line_height) as usize;
        if position.y < bounds.top() {
            line = 0;
        } else if line >= layout.line_starts.len() {
            line = layout.line_starts.len().saturating_sub(1);
        }
        let Some(shaped) = layout.lines.get(line) else {
            return self.content.len();
        };
        let line_start = layout.line_starts[line];
        let end = line_end(&self.content, &layout.line_starts, line);
        let display_index = shaped
            .closest_index_for_x(position.x - bounds.left())
            .min(shaped.len());
        line_start
            + display_to_content_index(&self.content[line_start..end], display_index, self.masked)
    }

    fn vertical_move(&mut self, delta: isize, extend: bool, cx: &mut Context<Self>) {
        if !self.multiline {
            return;
        }
        let offset = {
            let Some(layout) = self.last_layout.as_ref() else {
                return;
            };
            let cursor = self.cursor_offset();
            let line = line_for_offset(&layout.line_starts, cursor);
            let start = layout.line_starts[line];
            let end = line_end(&self.content, &layout.line_starts, line);
            let Some(shaped) = layout.lines.get(line) else {
                return;
            };
            let x = cursor_x(
                shaped,
                &self.content[start..end],
                (cursor - start).min(end - start),
                self.masked,
                layout.line_height,
            );
            let target = if delta < 0 {
                if line == 0 {
                    if extend {
                        self.select_to(0, cx);
                    } else {
                        self.move_to(0, cx);
                    }
                    return;
                }
                line - 1
            } else {
                if line + 1 >= layout.line_starts.len() {
                    if extend {
                        self.select_to(self.content.len(), cx);
                    } else {
                        self.move_to(self.content.len(), cx);
                    }
                    return;
                }
                line + 1
            };
            let target_start = layout.line_starts[target];
            let target_end = line_end(&self.content, &layout.line_starts, target);
            let Some(target_line) = layout.lines.get(target) else {
                return;
            };
            let display_index = target_line.closest_index_for_x(x).min(target_line.len());
            target_start
                + display_to_content_index(
                    &self.content[target_start..target_end],
                    display_index,
                    self.masked,
                )
        };
        if extend {
            self.select_to(offset, cx);
        } else {
            self.move_to(offset, cx);
        }
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify()
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;

        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }

        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;

        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }

        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }
}

fn line_starts(content: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (index, ch) in content.char_indices() {
        if ch == '\n' {
            starts.push(index + 1);
        }
    }
    starts
}

fn line_for_offset(starts: &[usize], offset: usize) -> usize {
    starts
        .partition_point(|start| *start <= offset)
        .saturating_sub(1)
}

fn line_end(content: &str, starts: &[usize], line: usize) -> usize {
    starts
        .get(line + 1)
        .map(|next_start| next_start.saturating_sub(1))
        .unwrap_or(content.len())
}

fn display_to_content_index(line: &str, display_index: usize, masked: bool) -> usize {
    if masked {
        line.char_indices()
            .nth(display_index)
            .map(|(index, _)| index)
            .unwrap_or(line.len())
    } else {
        display_index.min(line.len())
    }
}

fn cursor_x(
    line: &ShapedLine,
    line_content: &str,
    local_offset: usize,
    masked: bool,
    _line_height: Pixels,
) -> Pixels {
    let display_offset = if masked {
        line_content[..local_offset.min(line_content.len())]
            .chars()
            .count()
            .min(line.len())
    } else {
        local_offset.min(line.len())
    };
    line.x_for_index(display_offset)
}

fn runs_for_line(run: &TextRun, len: usize, marked: Option<Range<usize>>) -> Vec<TextRun> {
    if len == 0 {
        return Vec::new();
    }
    let Some(marked) = marked else {
        return vec![TextRun { len, ..run.clone() }];
    };
    let mark_start = marked.start.min(len);
    let mark_end = marked.end.min(len);
    if mark_start >= mark_end {
        return vec![TextRun { len, ..run.clone() }];
    }
    let mut runs = Vec::new();
    if mark_start > 0 {
        runs.push(TextRun {
            len: mark_start,
            ..run.clone()
        });
    }
    runs.push(TextRun {
        len: mark_end - mark_start,
        underline: Some(UnderlineStyle {
            color: Some(run.color),
            thickness: px(1.0),
            wavy: false,
        }),
        ..run.clone()
    });
    if mark_end < len {
        runs.push(TextRun {
            len: len - mark_end,
            ..run.clone()
        });
    }
    runs
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.marked_range.take();
        self.last_layout = None;
        cx.notify();
        if let Some(callback) = self.on_change.as_ref() {
            callback(&self.content, window, cx);
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        if !new_text.is_empty() {
            self.marked_range = Some(range.start..range.start + new_text.len());
        } else {
            self.marked_range = None;
        }
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .map(|new_range| new_range.start + range.start..new_range.end + range.end)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());

        self.last_layout = None;
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        let start_line = line_for_offset(&layout.line_starts, range.start);
        let end_line = line_for_offset(&layout.line_starts, range.end);
        let start_shaped = layout.lines.get(start_line)?;
        let end_shaped = layout.lines.get(end_line)?;
        let start_start = layout.line_starts[start_line];
        let start_end = line_end(&self.content, &layout.line_starts, start_line);
        let end_start = layout.line_starts[end_line];
        let end_end = line_end(&self.content, &layout.line_starts, end_line);
        let start_x = cursor_x(
            start_shaped,
            &self.content[start_start..start_end],
            (range.start - start_start).min(start_end - start_start),
            self.masked,
            layout.line_height,
        );
        let end_x = cursor_x(
            end_shaped,
            &self.content[end_start..end_end],
            (range.end - end_start).min(end_end - end_start),
            self.masked,
            layout.line_height,
        );
        let line_height = layout.line_height;
        Some(Bounds::from_corners(
            point(
                bounds.left() + start_x,
                bounds.top() + line_height * start_line as f32,
            ),
            point(
                bounds.left() + end_x,
                bounds.top() + line_height * (end_line as f32 + 1.),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        let layout = self.last_layout.as_ref()?;
        if layout.lines.is_empty() {
            return None;
        }
        let local = bounds.localize(&point)?;
        let line_height = layout.line_height.max(px(1.));
        let line = ((local.y / line_height) as usize).min(layout.line_starts.len() - 1);
        let shaped = layout.lines.get(line)?;
        let start = layout.line_starts[line];
        let end = line_end(&self.content, &layout.line_starts, line);
        let display_index = shaped.closest_index_for_x(local.x).min(shaped.len());
        let content_index =
            start + display_to_content_index(&self.content[start..end], display_index, self.masked);
        Some(self.offset_to_utf16(content_index))
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    lines: Vec<ShapedLine>,
    line_starts: Vec<usize>,
    line_height: Pixels,
    cursor: Option<PaintQuad>,
    selections: Vec<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let input = self.input.read(cx);
        let rows = if input.multiline {
            input.rows.max(1)
        } else {
            1
        };
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = (window.line_height() * rows as f32).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let content = input.content.clone();
        let starts = line_starts(&content);
        let line_height = window.line_height();
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let content_color: Hsla = theme::text_primary().into();
        let placeholder = content.is_empty();
        let base_color = if placeholder {
            theme::text_muted().into()
        } else {
            content_color
        };
        let run = TextRun {
            len: 0,
            font: style.font(),
            color: base_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };

        let mut lines = Vec::with_capacity(starts.len());
        for (line, start) in starts.iter().copied().enumerate() {
            let end = line_end(&content, &starts, line);
            let display: SharedString = if placeholder {
                input.placeholder.clone()
            } else if input.masked {
                "*".repeat(content[start..end].chars().count()).into()
            } else {
                content[start..end].to_owned().into()
            };
            let marked = if input.masked {
                None
            } else {
                input.marked_range.as_ref().and_then(|marked| {
                    let start = marked.start.max(start);
                    let end = marked.end.min(end);
                    (start < end).then_some(start - starts[line]..end - starts[line])
                })
            };
            let runs = runs_for_line(&run, display.len(), marked);
            let shaped = window
                .text_system()
                .shape_line(display, font_size, &runs, None);
            lines.push(shaped);
        }

        let cursor_offset = input.cursor_offset().min(content.len());
        let cursor_line = line_for_offset(&starts, cursor_offset);
        let cursor_start = starts[cursor_line];
        let cursor_end = line_end(&content, &starts, cursor_line);
        let cursor = lines.get(cursor_line).map(|shaped| {
            let x = cursor_x(
                shaped,
                &content[cursor_start..cursor_end],
                (cursor_offset - cursor_start).min(cursor_end - cursor_start),
                input.masked,
                line_height,
            );
            fill(
                Bounds::new(
                    point(
                        bounds.left() + x,
                        bounds.top() + line_height * cursor_line as f32,
                    ),
                    size(px(1.5), line_height),
                ),
                theme::accent(),
            )
        });

        let mut selections = Vec::new();
        if !input.selected_range.is_empty() {
            for (line, start) in starts.iter().copied().enumerate() {
                let end = line_end(&content, &starts, line);
                let selection_start = input.selected_range.start.max(start);
                let selection_end = input.selected_range.end.min(end);
                if selection_start >= selection_end {
                    continue;
                }
                let Some(shaped) = lines.get(line) else {
                    continue;
                };
                let x1 = cursor_x(
                    shaped,
                    &content[start..end],
                    selection_start - start,
                    input.masked,
                    line_height,
                );
                let x2 = if input.selected_range.end > end {
                    shaped.width.max(px(2.))
                } else {
                    cursor_x(
                        shaped,
                        &content[start..end],
                        selection_end - start,
                        input.masked,
                        line_height,
                    )
                };
                if x2 <= x1 {
                    continue;
                }
                selections.push(fill(
                    Bounds::new(
                        point(bounds.left() + x1, bounds.top() + line_height * line as f32),
                        size(x2 - x1, line_height),
                    ),
                    rgba(0x5B8CFF38),
                ));
            }
        }

        PrepaintState {
            lines,
            line_starts: starts,
            line_height,
            cursor: input.selected_range.is_empty().then_some(cursor).flatten(),
            selections,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        for selection in prepaint.selections.drain(..) {
            window.paint_quad(selection);
        }
        for (line, shaped) in prepaint.lines.iter().enumerate() {
            let _ = shaped.paint(
                point(
                    bounds.left(),
                    bounds.top() + prepaint.line_height * line as f32,
                ),
                prepaint.line_height,
                window,
                cx,
            );
        }

        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }

        let lines = std::mem::take(&mut prepaint.lines);
        let line_starts = std::mem::take(&mut prepaint.line_starts);
        let line_height = prepaint.line_height;
        self.input.update(cx, |input, _cx| {
            input.last_layout = Some(TextLayout {
                lines,
                line_starts,
                line_height,
            });
            input.last_bounds = Some(bounds);
        });
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus_handle.is_focused(window);
        let has_error = self.error.is_some();
        let error = self.error.clone();
        let rows = self.rows.max(1);
        let height = if self.multiline {
            window.line_height() * rows as f32 + px(16.)
        } else {
            px(32.)
        };
        let mut input = div()
            .flex()
            .w_full()
            .h(height)
            .px(px(10.))
            .bg(theme::bg_sunken())
            .border_1()
            .border_color(if has_error {
                theme::danger()
            } else if focused {
                theme::accent()
            } else {
                theme::border_default()
            })
            .rounded(px(6.))
            .overflow_hidden()
            .key_context("TextInput")
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .text_size(px(13.))
            .line_height(px(18.))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::insert_newline))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .child(TextElement { input: cx.entity() });
        input = if self.multiline {
            input.items_start().py(px(8.))
        } else {
            input.items_center()
        };

        div()
            .flex()
            .flex_col()
            .gap_1()
            .w_full()
            .child(input)
            .when_some(error, |this, error| {
                this.child(
                    div()
                        .text_size(px(11.))
                        .text_color(theme::danger())
                        .child(error),
                )
            })
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
