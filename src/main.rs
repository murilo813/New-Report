#![windows_subsystem = "windows"]
#![allow(non_snake_case)]

mod core;
mod views;
mod components {
    pub mod error_modal;
}
use crate::core::engine::DataEngine;
use dioxus::desktop::{Config, WindowBuilder};
use dioxus::prelude::*;
use std::path::Path;
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
    dotenvy::dotenv().ok();

    let config = Config::new().with_window(
        WindowBuilder::new()
            .with_title("NewREPORT - Agro Zecão")
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

            let loaded_engine = tokio::task::spawn_blocking(|| DataEngine::new())
                .await
                .unwrap();
            engine_signal.set(loaded_engine);
            is_loaded.set(true);
        }
    });

    if !is_loaded() {
        return rsx! {
            div {
                style: "display: flex; height: 100vh; width: 100vw; background-color: var(--bg-color, #1e1e1e); color: var(--text-color, #fff); align-items: center; justify-content: center; flex-direction: column; gap: 15px; font-family: sans-serif;",
                h2 { "Iniciando New Report..." }
                p { style: "color: gray;", "Carregando estrutura de tabelas..." }
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
            on_back: move |_: MouseEvent| current_route.set(Route::Home)
          }
        },
    };

    rsx! {
      style { {include_str!("style/main.css")} }
      {content}
    }
}
