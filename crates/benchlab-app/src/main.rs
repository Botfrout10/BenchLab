//! BenchLab desktop entry point.

mod demo;
mod mock;
mod services;
mod test_request;

use std::{process::ExitCode, sync::Arc};

use benchlab_core::{APP_NAME, APP_VERSION};
use benchlab_engine::detect_k6;
use benchlab_ui::{
    backend::BenchLabBackend,
    components::{
        ButtonSize, ButtonVariant, button,
        text_input::{
            Backspace, Copy, Cut, Delete, Down, End, Enter, Home, Left, Paste, Right, SelectAll,
            SelectDown, SelectLeft, SelectRight, SelectUp, Up,
        },
    },
    shell::{CloseModal, Shell},
    theme,
};
use gpui::{
    App, AppContext, Application, Bounds, Context, FontWeight, KeyBinding, Render, TitlebarOptions,
    WindowBounds, WindowOptions, div, prelude::*, px, size,
};
use services::{AppInstance, boot};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|arg| arg == "--version") {
        println!("{APP_NAME} {APP_VERSION}");
        return ExitCode::SUCCESS;
    }

    if args.iter().any(|arg| arg == "--check") {
        return check_environment();
    }

    match boot() {
        Ok(instance) => {
            run_app(instance);
            ExitCode::SUCCESS
        }
        Err(error) => {
            show_startup_error(error);
            ExitCode::FAILURE
        }
    }
}

fn check_environment() -> ExitCode {
    let status = detect_k6();

    println!("{APP_NAME} {APP_VERSION}");
    println!("engine: {}", status.summary());
    if let Some(version) = status.version.as_deref() {
        println!("version: {version}");
    }
    if let Some(path) = status.path.as_ref() {
        println!("path: {}", path.display());
    }

    if status.ready() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    }
}

fn run_app(instance: AppInstance) {
    let services = instance.services.clone();
    let backend: Arc<dyn BenchLabBackend> = Arc::new(services.clone());
    let view = services.view();

    Application::new().run(move |cx: &mut App| {
        let _keep_alive = &instance;
        bind_keys(cx);

        let bounds = Bounds::centered(None, size(px(1240.), px(760.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("BenchLab — API performance workbench".into()),
                        ..Default::default()
                    }),
                    window_min_size: Some(size(px(1080.), px(720.))),
                    ..Default::default()
                },
                {
                    let backend = backend.clone();
                    let view = view.clone();
                    move |_window, cx| cx.new(|cx| Shell::new(backend, view, cx))
                },
            )
            .expect("failed to open the BenchLab window");

        window
            .update(cx, |shell, window, cx| {
                shell.focus_search(window, cx);
                cx.activate(true);
            })
            .expect("failed to initialize the BenchLab window");
    });
}

fn show_startup_error(message: String) {
    eprintln!("{message}");
    let message = Arc::new(message);
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(560.), px(300.)), cx);
        let message = message.clone();
        let result = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("BenchLab".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |_window, cx| cx.new(|_cx| StartupErrorView { message }),
        );
        if result.is_err() {
            eprintln!("Could not show the BenchLab startup error window.");
        }
    });
}

struct StartupErrorView {
    message: Arc<String>,
}

impl Render for StartupErrorView {
    fn render(&mut self, _window: &mut gpui::Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .size_full()
            .p(px(24.))
            .bg(theme::bg_app())
            .text_color(theme::text_primary())
            .child(
                div()
                    .text_size(px(18.))
                    .font_weight(FontWeight::BOLD)
                    .child("BenchLab could not start"),
            )
            .child(
                div()
                    .id("startup-error-message")
                    .flex_1()
                    .overflow_y_scroll()
                    .text_size(px(12.))
                    .text_color(theme::text_secondary())
                    .child(self.message.as_str().to_owned()),
            )
            .child(div().flex().justify_end().child(
                button("Close", ButtonVariant::Primary, ButtonSize::Sm).on_mouse_down(
                    gpui::MouseButton::Left,
                    |_: &gpui::MouseDownEvent, _: &mut gpui::Window, cx: &mut App| {
                        cx.quit();
                    },
                ),
            ))
    }
}

fn bind_keys(cx: &mut App) {
    let text_context = Some("TextInput");
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, text_context),
        KeyBinding::new("delete", Delete, text_context),
        KeyBinding::new("left", Left, text_context),
        KeyBinding::new("right", Right, text_context),
        KeyBinding::new("up", Up, text_context),
        KeyBinding::new("down", Down, text_context),
        KeyBinding::new("shift-left", SelectLeft, text_context),
        KeyBinding::new("shift-right", SelectRight, text_context),
        KeyBinding::new("shift-up", SelectUp, text_context),
        KeyBinding::new("shift-down", SelectDown, text_context),
        KeyBinding::new("home", Home, text_context),
        KeyBinding::new("end", End, text_context),
        KeyBinding::new("enter", Enter, text_context),
        KeyBinding::new("ctrl-a", SelectAll, text_context),
        KeyBinding::new("ctrl-c", Copy, text_context),
        KeyBinding::new("ctrl-x", Cut, text_context),
        KeyBinding::new("ctrl-v", Paste, text_context),
        KeyBinding::new("cmd-a", SelectAll, text_context),
        KeyBinding::new("cmd-c", Copy, text_context),
        KeyBinding::new("cmd-x", Cut, text_context),
        KeyBinding::new("cmd-v", Paste, text_context),
        KeyBinding::new("escape", CloseModal, Some("BenchLabShell")),
    ]);
}
