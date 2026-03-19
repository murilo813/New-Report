#![windows_subsystem = "windows"]
#![allow(non_snake_case)]

mod core;
mod views;
mod components {
    pub mod status_modal;
}
use crate::core::engine::DataEngine;
use dioxus::desktop::{Config, WindowBuilder};
use dioxus::prelude::*;
use std::path::Path;
use std::env;
use views::editor::EditQuery;
use views::home::Home;
use views::report::ViewReport;

#[derive(Clone, Copy, PartialEq)]
enum Route {
    Home,
    ViewReport,
    EditQuery,
}

fn main() {
    if let Ok(exe_path) = env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let runtime_path = exe_dir.join("webview_runtime");
            
            if runtime_path.exists() {
                unsafe {
                    env::set_var("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER", runtime_path.to_str().unwrap());
                }
            }
        }
    }

    dotenvy::dotenv().ok();

    let config = Config::new().with_window(
        WindowBuilder::new()
            .with_title("NewREPORT - Agro Zecão | Powered by DataFusion")
            .with_maximized(true),
    );

    LaunchBuilder::desktop().with_cfg(config).launch(App);
}

fn App() -> Element {
    let mut is_loaded = use_signal(|| false);
    let mut current_route = use_signal(|| Route::Home);
    let mut selected_report = use_signal(|| String::new());

    let mut engine_signal = use_signal(|| DataEngine::new_empty());
    let current_sql_signal = use_signal(|| String::new());

    use_future(move || async move {
        if !is_loaded() {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;

            let engine_result = tokio::task::spawn_blocking(|| DataEngine::new()).await;

            match engine_result {
                Ok(loaded_engine) => {
                    DataEngine::start_background_warming(
                        loaded_engine.base_path.clone(),
                        "relatorios".to_string(),
                        loaded_engine.active_tables.clone()
                    );

                    engine_signal.set(loaded_engine);
                    is_loaded.set(true);
                }
                Err(e) => {
                    println!("Erro crítico ao subir a Engine: {:?}", e);
                }
            }
        }
    });

    if !is_loaded() {
        return rsx! {
            div { class: "loading-screen",
                div { class: "spinner" }
                h2 { "Iniciando Engine de Alta Performance..." }
                p { class: "loading-subtitle", "Subindo motor analítico DataFusion..." }
            }
        };
    }

    let content: Element = match current_route() {
        Route::Home => rsx! {
            Home {
                selected_name: selected_report(),
                on_select: move |name: String| selected_report.set(name),
                on_open: move |_| current_route.set(Route::ViewReport),
                on_edit: move |_| {
                    let current = selected_report.read();
                    if current.contains(".json") || Path::new(&*current).exists() {
                        current_route.set(Route::EditQuery);
                    }
                },
                engine: engine_signal,
                current_sql: current_sql_signal
            }
        },
        Route::ViewReport => rsx! {
            ViewReport {
                on_back: move |_: MouseEvent| current_route.set(Route::Home),
                engine: engine_signal,
                query_sql: current_sql_signal()
            }
        },
        Route::EditQuery => rsx! {
            EditQuery {
                report_name: selected_report(),
                engine: engine_signal,
                on_back: move |_: MouseEvent| current_route.set(Route::Home)
            }
        },
    };

    rsx! {
        style { {include_str!("style/main.css")} }
        {content}
    }
}
