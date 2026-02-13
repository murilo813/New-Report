use crate::components::error_modal::SqlErrorModal;
use crate::core::engine::DataEngine;
use dioxus::prelude::*;
use serde::Deserialize;
use std::fs;
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

#[derive(Deserialize)]
struct ReportConfig {
    query_sql: String,
}

#[derive(Debug, Clone)]
struct FileItem {
    name: String,
    path_str: String,
    is_dir: bool,
    children: Vec<FileItem>,
}

enum LoaderMsg {
    Progress(f32),
    Finished(DataEngine, String),
    Error(String),
}

fn read_reports(dir: &Path) -> Vec<FileItem> {
    let mut items = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let path_str = path.to_string_lossy().to_string();
            let is_dir = path.is_dir();
            let children = if is_dir {
                read_reports(&path)
            } else {
                if path.extension().map_or(false, |ext| ext == "json") {
                    Vec::new()
                } else {
                    continue;
                }
            };
            items.push(FileItem {
                name,
                path_str,
                is_dir,
                children,
            });
        }
    }
    items.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    items
}

fn filter_nodes(items: &[FileItem], query: &str) -> Vec<FileItem> {
    if query.is_empty() {
        return items.to_vec();
    }
    let lower_query = query.to_lowercase();
    items
        .iter()
        .filter_map(|item| {
            if item.is_dir {
                let filtered_children = filter_nodes(&item.children, query);
                if !filtered_children.is_empty() || item.name.to_lowercase().contains(&lower_query)
                {
                    let mut new_dir = item.clone();
                    new_dir.children = filtered_children;
                    return Some(new_dir);
                }
            } else if item.name.to_lowercase().contains(&lower_query) {
                return Some(item.clone());
            }
            None
        })
        .collect()
}

fn count_files(items: &[FileItem]) -> usize {
    items
        .iter()
        .map(|item| {
            if item.is_dir {
                count_files(&item.children)
            } else {
                1
            }
        })
        .sum()
}

#[component]
pub fn Home(
    selected_name: String,
    on_select: EventHandler<String>,
    on_open: EventHandler<()>,
    on_edit: EventHandler<()>,
    mut engine: Signal<DataEngine>,
    mut current_sql: Signal<String>,
) -> Element {
    let mut reports = use_signal(|| read_reports(Path::new("relatorios")));
    let mut search_text = use_signal(|| String::new());
    let mut show_error = use_signal(|| false);
    let mut error_msg = use_signal(|| String::new());
    let last_sql = use_signal(|| String::new());

    let mut is_loading = use_signal(|| false);
    let mut progress = use_signal(|| 0.0f32);
    let cancel_flag = use_signal(|| Arc::new(AtomicBool::new(false)));

    let filtered_items = filter_nodes(&reports.read(), &search_text.read());
    let total_files = count_files(&filtered_items);

    let mut tx_signal = use_signal(|| None::<mpsc::Sender<LoaderMsg>>);
    let mut rx_signal = use_signal(|| None::<mpsc::Receiver<LoaderMsg>>);

    use_future(move || async move {
        loop {
            if is_loading() {
                if let Some(rx) = rx_signal.read().as_ref() {
                    while let Ok(msg) = rx.try_recv() {
                        match msg {
                            LoaderMsg::Progress(p) => progress.set(p),
                            LoaderMsg::Error(e) => {
                                error_msg.set(e);
                                show_error.set(true);
                                is_loading.set(false);
                            }
                            LoaderMsg::Finished(new_engine, sql) => {
                                engine.set(new_engine);
                                current_sql.set(sql);
                                is_loading.set(false);
                                on_open.call(());
                            }
                        }
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(16)).await;
        }
    });

    let open_report = move |path_to_open: String| {
        let content = match fs::read_to_string(&path_to_open) {
            Ok(c) => c,
            Err(e) => {
                error_msg.set(format!("Erro ao ler: {}", e));
                show_error.set(true);
                return;
            }
        };

        let config: ReportConfig = match serde_json::from_str(&content) {
            Ok(c) => c,
            Err(e) => {
                error_msg.set(format!("Erro JSON: {}", e));
                show_error.set(true);
                return;
            }
        };

        let (tx, rx) = mpsc::channel();
        tx_signal.set(Some(tx.clone()));
        rx_signal.set(Some(rx));

        is_loading.set(true);
        progress.set(0.0);
        cancel_flag.read().store(false, Ordering::SeqCst);

        let sql_to_process = config.query_sql.clone();
        let current_cancel = cancel_flag.read().clone();

        std::thread::spawn(move || {
            let mut new_engine = DataEngine::new();
            let tx_progress = tx.clone();

            let result = new_engine.process_report_with_progress(
                &sql_to_process,
                current_cancel,
                move |p| {
                    let _ = tx_progress.send(LoaderMsg::Progress(p));
                },
            );

            match result {
                Ok(_) => {
                    let _ = tx.send(LoaderMsg::Progress(100.0));
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    let _ = tx.send(LoaderMsg::Finished(new_engine, sql_to_process));
                }
                Err(err) => {
                    if err != "Operação cancelada pelo usuário" {
                        let _ = tx.send(LoaderMsg::Error(err));
                    }
                }
            }
        });
    };

    let on_cancel_click = move |_| {
        cancel_flag.read().store(true, Ordering::SeqCst);
        is_loading.set(false);
        progress.set(0.0);
    };

    let selected_name_delete = selected_name.clone();
    let delete_selected = move |_| {
        if selected_name_delete.is_empty() || selected_name_delete == "Nenhum selecionado" {
            return;
        }
        if let Ok(_) = fs::remove_file(&selected_name_delete) {
            reports.set(read_reports(Path::new("relatorios")));
            on_select.call(String::from("Nenhum selecionado"));
        }
    };

    rsx! {
        div { class: "app-container",
            SqlErrorModal {
                show: show_error,
                error_message: error_msg(),
                sql_content: last_sql(),
                on_close: move |_| show_error.set(false)
            }

            div { class: "middle-section",
                div { class: "sidebar",
                    button { class: "btn-classic", onclick: move |_| {
                        on_select.call(String::from("relatorios/novo_relatorio.json"));
                        on_edit.call(());
                    }, "✚ Novo" }
                    button { class: "btn-classic", onclick: move |_| on_edit.call(()), "✎ Editar" }
                    button { class: "btn-classic", onclick: delete_selected, "✖ Excluir" }
                }

                div { class: "main-view",
                    div { class: "top-toolbar",
                        input {
                            class: "input-classic search-input",
                            placeholder: "Pesquisa...",
                            value: "{search_text}",
                            oninput: move |evt| search_text.set(evt.value())
                        }
                        span { "Total: {total_files}" }
                        input { class: "input-classic selected-report-display", readonly: true, value: "{selected_name}" }
                    }
                    div { class: "tree-container",
                        ul { class: "tree-list",
                            {render_tree(&filtered_items, &selected_name, on_select, EventHandler::new(open_report))}
                        }
                    }
                }
            }

            div { class: "status-bar-container",
                span { class: "status-text", "Relatório atual: {selected_name}" }
                if is_loading() {
                    div { class: "loading-group-minimal",
                        span { class: "progress-label", "{*progress.read() as i32}%" }
                        div { class: "progress-bar-mini-bg",
                            div { class: "progress-bar-mini-fill", style: "width: {progress.read()}%" }
                        }
                        button { class: "btn-abort-link", onclick: on_cancel_click, "Cancelar" }
                    }
                }
            }
        }
    }
}

fn render_tree(
    items: &[FileItem],
    selected_path: &str,
    on_select: EventHandler<String>,
    on_open_named: EventHandler<String>,
) -> Element {
    rsx! {
        {items.iter().map(|item| {
            let name = item.name.clone();
            let path = item.path_str.clone();
            let is_dir = item.is_dir;
            let children = item.children.clone();
            let is_selected = selected_path == path;

            if is_dir {
                rsx! {
                    Fragment { key: "{path}",
                        li { span { class: "tree-icon", "📁" } span { class: "folder-name", "{name}" } }
                        ul { {render_tree(&children, selected_path, on_select.clone(), on_open_named.clone())} }
                    }
                }
            } else {
                let p_select = path.clone();
                let p_open = path.clone();
                rsx! {
                    li {
                        key: "{path}",
                        onclick: move |_| on_select.call(p_select.clone()),
                        ondoubleclick: move |_| on_open_named.call(p_open.clone()),
                        span { class: "tree-icon", "📊" }
                        span { class: if is_selected { "file-name selected" } else { "file-name" }, "{name}" }
                    }
                }
            }
        })}
    }
}
