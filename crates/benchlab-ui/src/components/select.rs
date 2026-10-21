//! A compact select for fixed option sets. Mouse selection and a clear selected
//! state; searchable/keyboard navigation arrives with the larger option sets in
//! a later milestone.

use gpui::{
    Context, CursorStyle, FocusHandle, Focusable, MouseButton, MouseDownEvent, Render,
    SharedString, Window, deferred, div, prelude::*, px,
};

use crate::theme;

type ChangeCallback = Box<dyn Fn(usize, &mut Window, &mut gpui::App)>;

pub struct Select {
    focus_handle: FocusHandle,
    options: Vec<SharedString>,
    selected: usize,
    open: bool,
    placeholder: SharedString,
    on_change: Option<ChangeCallback>,
}

impl Select {
    pub fn new(
        options: impl IntoIterator<Item = impl Into<SharedString>>,
        selected: usize,
        cx: &mut Context<Self>,
    ) -> Self {
        let options: Vec<SharedString> = options.into_iter().map(Into::into).collect();
        let selected = selected.min(options.len().saturating_sub(1));
        Self {
            focus_handle: cx.focus_handle(),
            options,
            selected,
            open: false,
            placeholder: "None".into(),
            on_change: None,
        }
    }

    pub fn set_on_change(&mut self, callback: Option<ChangeCallback>) {
        self.on_change = callback;
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn selected_label(&self) -> SharedString {
        self.options
            .get(self.selected)
            .cloned()
            .unwrap_or_else(|| self.placeholder.clone())
    }

    pub fn set_selected(&mut self, selected: usize, cx: &mut Context<Self>) {
        if self.selected != selected {
            self.selected = selected.min(self.options.len().saturating_sub(1));
            cx.notify();
        }
    }

    pub fn set_options(
        &mut self,
        options: impl IntoIterator<Item = impl Into<SharedString>>,
        cx: &mut Context<Self>,
    ) {
        self.options = options.into_iter().map(Into::into).collect();
        self.selected = self.selected.min(self.options.len().saturating_sub(1));
        self.open = false;
        cx.notify();
    }

    pub fn set_placeholder(
        &mut self,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.placeholder = placeholder.into();
        cx.notify();
    }

    fn toggle(&mut self, _: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle);
        self.open = !self.open;
        cx.notify();
    }

    fn choose(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = index;
        self.open = false;
        cx.notify();
        if let Some(callback) = self.on_change.as_ref() {
            callback(index, window, cx);
        }
    }
}

impl Render for Select {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.open;
        let selected = self.selected;
        let label = self.selected_label();
        let options: Vec<(usize, SharedString)> =
            self.options.iter().cloned().enumerate().collect();

        div()
            .relative()
            .w_full()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(32.))
                    .px(px(10.))
                    .bg(theme::bg_sunken())
                    .border_1()
                    .border_color(if open {
                        theme::accent()
                    } else {
                        theme::border_default()
                    })
                    .rounded(px(6.))
                    .cursor(CursorStyle::PointingHand)
                    .text_size(px(12.))
                    .text_color(theme::text_primary())
                    .track_focus(&self.focus_handle)
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::toggle))
                    .child(label)
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(theme::text_muted())
                            .child("▾"),
                    ),
            )
            .when(open, |this| {
                this.child(
                    deferred(
                        div()
                            .id("select-options")
                            .absolute()
                            .top(px(34.))
                            .left(px(0.))
                            .w_full()
                            .max_h(px(220.))
                            .overflow_y_scroll()
                            .bg(theme::bg_raised())
                            .border_1()
                            .border_color(theme::border_strong())
                            .rounded(px(6.))
                            .children(options.into_iter().map(|(index, option)| {
                                let is_selected = index == selected;
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .h(px(30.))
                                    .px(px(10.))
                                    .text_size(px(12.))
                                    .text_color(if is_selected {
                                        theme::text_primary()
                                    } else {
                                        theme::text_secondary()
                                    })
                                    .bg(if is_selected {
                                        theme::bg_raised()
                                    } else {
                                        theme::bg_surface()
                                    })
                                    .cursor(CursorStyle::PointingHand)
                                    .hover(|style| style.bg(theme::bg_raised()))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, window, cx| {
                                            this.choose(index, window, cx);
                                        }),
                                    )
                                    .child(option)
                                    .when(is_selected, |this| {
                                        this.child(
                                            div()
                                                .text_size(px(10.))
                                                .text_color(theme::accent())
                                                .child("✓"),
                                        )
                                    })
                            })),
                    )
                    .with_priority(100),
                )
            })
    }
}

impl Focusable for Select {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
