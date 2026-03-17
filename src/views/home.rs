use crate::components::status_modal::{StatusModal, StatusType};
use crate::core::engine::{DataEngine, append_log};
use crate::views::editor::ReportParameter;
use dioxus::prelude::*;
use serde::Deserialize;
use std::fs;
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

#[derive(Deserialize, Clone)]
struct ReportConfig {
    query_sql: String,
    #[serde(default)]
    parametros: Vec<ReportParameter>,
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

// LÓGICA DE SISTEMA DE ARQUIVOS
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

// COMPONENTES
#[component]
fn LogsModal(show: Signal<bool>, on_close: EventHandler<()>) -> Element {
    if !show() {
        return rsx! {};
    }

    let logs_content = std::fs::read_to_string("logs_desempenho.txt")
        .unwrap_or_else(|_| "Nenhum log de desempenho encontrado ainda...".to_string());

    rsx! {
        div { class: "modal-overlay overlay-dark",
            div { class: "modal-window modal-w80-h80",
                h2 { class: "logs-title", "📜 Histórico de Desempenho (Logs)" }
                textarea { class: "input-classic log-textarea", readonly: true, value: "{logs_content}" }
                button { class: "btn-classic btn-close-logs", onclick: move |_| on_close.call(()), "✖ Fechar Logs" }
            }
        }
    }
}

#[component]
fn LookupView(
    show: Signal<bool>,
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    active_param: Signal<String>,
    user_values: Signal<std::collections::HashMap<String, String>>,
) -> Element {
    if !show() {
        return rsx! {};
    }

    rsx! {
        div { class: "modal-overlay overlay-lookup",
            div { class: "modal-window modal-w600",
                div { class: "modal-header", "Selecione uma opção" }
                div { class: "modal-body modal-body-lookup",
                    table { class: "pg-table table-wrapper",
                        thead { tr { {headers.iter().map(|h| rsx! { th { key: "{h}", class: "sticky-header", "{h}" } })} } }
                        tbody {
                            {rows.iter().enumerate().map(|(i, row)| {
                                let row_id = row.get(0).cloned().unwrap_or_default();
                                rsx! {
                                    tr {
                                        key: "{i}", class: "cursor-pointer border-b-color",
                                        onclick: move |_| {
                                            user_values.write().insert(active_param(), row_id.clone());
                                            show.set(false);
                                        },
                                        {row.iter().map(|cell| rsx! { td { class: "p-8", "{cell}" } })}
                                    }
                                }
                            })}
                        }
                    }
                    if rows.is_empty() { div { class: "lookup-empty", "Nenhum resultado encontrado." } }
                }
                div { class: "modal-footer",
                    button { class: "btn-classic", onclick: move |_| show.set(false), "Cancelar" }
                }
            }
        }
    }
}

#[component]
fn ParamsModal(
    show: Signal<bool>,
    report_config: Signal<Option<ReportConfig>>,
    report_path: Signal<String>,
    on_close: EventHandler<()>,
    on_generate: EventHandler<(String, String)>,
) -> Element {
    if !show() {
        return rsx! {};
    }

    let config = match report_config.read().clone() {
        Some(c) => c,
        None => return rsx! {},
    };

    let params_for_generate = config.parametros.clone();
    let query_for_generate = config.query_sql.clone();

    let mut user_values = use_signal(|| {
        let mut map = std::collections::HashMap::new();
        for p in &config.parametros {
            let default_val = if p.tipo == "bool" {
                "false".to_string()
            } else {
                p.valor_padrao.clone()
            };
            map.insert(p.id.clone(), default_val);
        }
        map
    });

    let mut validation_error = use_signal(|| String::new());
    let mut show_lookup = use_signal(|| false);
    let mut lookup_data = use_signal(|| (Vec::<String>::new(), Vec::<Vec<String>>::new()));
    let mut lookup_active_param = use_signal(|| String::new());
    let mut is_lookup_loading = use_signal(|| false);

    let handle_generate = move |_| {
        let mut final_sql = query_for_generate.clone();
        for p in &params_for_generate {
            let val = user_values.read().get(&p.id).cloned().unwrap_or_default();
            if p.requerido && val.trim().is_empty() {
                validation_error.set(format!("O campo '{}' é obrigatório.", p.nome));
                return;
            }
            final_sql = final_sql.replace(&format!("[{}]", p.id), &val);
        }
        validation_error.set(String::new());
        on_generate.call((report_path.read().clone(), final_sql));
    };

    let params_list = config.parametros.clone();
    let (lookup_headers, lookup_rows) = lookup_data.read().clone();

    rsx! {
        div { class: "modal-overlay",
            div { class: "modal-window modal-w400",
                div { class: "modal-header", "Parâmetros do Relatório" }

                div { class: "modal-body modal-body-scrollable",
                    if !validation_error().is_empty() {
                        div { class: "error-message-box", "{validation_error}" }
                    }

                    if params_list.is_empty() {
                        p { class: "empty-msg", "Este relatório não requer parâmetros." }
                    } else {
                        {params_list.iter().map(|p| {
                            let p_id = p.id.clone();
                            let current_val = user_values.read().get(&p.id).cloned().unwrap_or_default();

                            rsx! {
                                div { class: "form-group", key: "{p.id}",
                                    label { "{p.nome}" if p.requerido { span { class: "input-required-mark", " *" } } }

                                    if p.tipo == "bool" {
                                        select { class: "input-classic input-h30", value: "{current_val}", onchange: move |evt| { user_values.write().insert(p_id.clone(), evt.value()); },
                                            option { value: "false", "Não" } option { value: "true", "Sim" }
                                        }
                                    } else if p.tipo == "data" {
                                        input { class: "input-classic input-h30", r#type: "date", value: "{current_val}", oninput: move |evt| { user_values.write().insert(p_id.clone(), evt.value()); } }
                                    } else if p.tipo == "int" || p.tipo == "float" {
                                        input { class: "input-classic input-h30", r#type: "number", value: "{current_val}", oninput: move |evt| { user_values.write().insert(p_id.clone(), evt.value()); } }
                                    } else if p.tipo == "pesquisa" {
                                        div { class: "flex-row-gap5",
                                            input { class: "input-classic input-h30 flex-1", r#type: "text", value: "{current_val}", oninput: move |evt| { user_values.write().insert(p_id.clone(), evt.value()); } }
                                            button {
                                                class: "btn-classic btn-icon-small",
                                                disabled: *is_lookup_loading.read() && *lookup_active_param.read() == p.id,
                                                onclick: {
                                                    let param_id = p.id.clone(); let extra_sql = p.extra.clone();
                                                    move |_| {
                                                        if extra_sql.trim().is_empty() { validation_error.set("A query de pesquisa está vazia.".to_string()); return; }
                                                        validation_error.set(String::new());
                                                        lookup_active_param.set(param_id.clone());
                                                        is_lookup_loading.set(true);

                                                        let sql_for_spawn = extra_sql.clone();
                                                        spawn(async move {
                                                            let (tx, rx) = std::sync::mpsc::channel();
                                                            std::thread::spawn(move || {
                                                                let mut tmp_engine = DataEngine::new();
                                                                let cancel = Arc::new(AtomicBool::new(false));
                                                                if let Ok(_) = tmp_engine.process_report_with_progress(&sql_for_spawn, cancel, |_| {}) {
                                                                    if let Ok(mut stmt) = tmp_engine.execute_user_sql(&sql_for_spawn) {
                                                                        let cols: Vec<String> = stmt.column_names().into_iter().map(|s| s.to_string()).collect();
                                                                        let col_count = stmt.column_count();
                                                                        let rows_iter = stmt.query_map([], |row| {
                                                                            let mut r = Vec::with_capacity(col_count);
                                                                            for i in 0..col_count {
                                                                                let val = row.get_ref(i).unwrap();
                                                                                r.push(match val {
                                                                                    rusqlite::types::ValueRef::Null => "".to_string(),
                                                                                    rusqlite::types::ValueRef::Integer(v) => v.to_string(),
                                                                                    rusqlite::types::ValueRef::Real(v) => format!("{:.2}", v),
                                                                                    rusqlite::types::ValueRef::Text(v) => String::from_utf8_lossy(v).to_string(),
                                                                                    _ => "[BIN]".to_string(),
                                                                                });
                                                                            }
                                                                            Ok(r)
                                                                        }).unwrap();
                                                                        let rows: Vec<Vec<String>> = rows_iter.filter_map(|r| r.ok()).collect();
                                                                        let _ = tx.send(Ok((cols, rows)));
                                                                        return;
                                                                    }
                                                                }
                                                                let _ = tx.send(Err("Erro ao processar SQL de pesquisa.".to_string()));
                                                            });
                                                            if let Ok(res) = rx.recv() {
                                                                match res { Ok(data) => { lookup_data.set(data); show_lookup.set(true); } Err(e) => validation_error.set(e), }
                                                            }
                                                            is_lookup_loading.set(false);
                                                        });
                                                    }
                                                },
                                                if *is_lookup_loading.read() && *lookup_active_param.read() == p.id { "⏳" } else { "🔍" }
                                            }
                                        }
                                    } else {
                                        input { class: "input-classic input-h30", r#type: "text", value: "{current_val}", oninput: move |evt| { user_values.write().insert(p_id.clone(), evt.value()); } }
                                    }
                                }
                            }
                        })}
                    }
                }

                div { class: "modal-footer",
                    button { class: "btn-classic flex-1", onclick: move |_| on_close.call(()), "Cancelar" }
                    button { class: "btn-classic btn-primary flex-1", onclick: handle_generate, "Gerar Relatório" }
                }
            }

            LookupView { show: show_lookup, headers: lookup_headers, rows: lookup_rows, active_param: lookup_active_param, user_values: user_values }
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
            let name = item.name.clone(); let path = item.path_str.clone();
            let is_selected = selected_path == path;

            if item.is_dir {
                rsx! {
                    Fragment { key: "{path}",
                        li { span { class: "tree-icon", "📁" } span { class: "folder-name", "{name}" } }
                        ul { {render_tree(&item.children, selected_path, on_select.clone(), on_open_named.clone())} }
                    }
                }
            } else {
                let p_select = path.clone(); let p_open = path.clone();
                rsx! {
                    li { key: "{path}", onclick: move |_| on_select.call(p_select.clone()), ondoubleclick: move |_| on_open_named.call(p_open.clone()),
                        span { class: "tree-icon", "📊" } span { class: if is_selected { "file-name selected" } else { "file-name" }, "{name}" }
                    }
                }
            }
        })}
    }
}

// CONTROLLER
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
    let mut show_status_modal = use_signal(|| false);
    let mut status_modal_type = use_signal(|| StatusType::Error);
    let mut status_msg = use_signal(|| String::new());
    let last_sql = use_signal(|| String::new());
    let mut show_logs = use_signal(|| false);

    let mut show_params_modal = use_signal(|| false);
    let mut current_report_config = use_signal(|| None::<ReportConfig>);
    let mut current_report_path = use_signal(|| String::new());

    let mut is_loading = use_signal(|| false);
    let mut progress = use_signal(|| 0.0f32);
    let cancel_flag = use_signal(|| Arc::new(AtomicBool::new(false)));
    let mut tx_signal = use_signal(|| None::<mpsc::Sender<LoaderMsg>>);
    let mut rx_signal = use_signal(|| None::<mpsc::Receiver<LoaderMsg>>);

    let filtered_items = filter_nodes(&reports.read(), &search_text.read());
    let total_files = count_files(&filtered_items);

    use_future(move || async move {
        loop {
            if is_loading() {
                if let Some(rx) = rx_signal.read().as_ref() {
                    while let Ok(msg) = rx.try_recv() {
                        match msg {
                            LoaderMsg::Progress(p) => progress.set(p),
                            LoaderMsg::Error(e) => {
                                status_msg.set(e);
                                status_modal_type.set(StatusType::Error);
                                show_status_modal.set(true);
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

    let prepare_report = move |path_to_open: String| {
        let content = match fs::read_to_string(&path_to_open) {
            Ok(c) => c,
            Err(e) => {
                status_msg.set(format!("Erro ao ler: {}", e));
                status_modal_type.set(StatusType::Error);
                show_status_modal.set(true);
                return;
            }
        };
        let config: ReportConfig = match serde_json::from_str(&content) {
            Ok(c) => c,
            Err(e) => {
                status_msg.set(format!("Erro JSON: {}", e));
                status_modal_type.set(StatusType::Error);
                show_status_modal.set(true);
                return;
            }
        };
        current_report_config.set(Some(config));
        current_report_path.set(path_to_open);
        show_params_modal.set(true);
    };

    let execute_report = move |(path_to_open, final_sql): (String, String)| {
        show_params_modal.set(false);
        let (tx, rx) = mpsc::channel();
        tx_signal.set(Some(tx.clone()));
        rx_signal.set(Some(rx));
        is_loading.set(true);
        progress.set(0.0);
        cancel_flag.read().store(false, Ordering::SeqCst);

        let sql_to_process = final_sql.clone();
        let current_cancel = cancel_flag.read().clone();
        let report_name_log = path_to_open.clone();

        std::thread::spawn(move || {
            let mut new_engine = DataEngine::new();
            let tx_progress = tx.clone();
            let start_time = std::time::Instant::now();

            let result = new_engine.process_report_with_progress(
                &sql_to_process,
                current_cancel,
                move |p| {
                    let _ = tx_progress.send(LoaderMsg::Progress(p));
                },
            );
            let elapsed_ms = start_time.elapsed().as_millis();
            if result.is_ok() {
                append_log(
                    &report_name_log,
                    "Processamento DBISAM -> RAM SQLite",
                    elapsed_ms,
                );
            }

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

    let delete_selected = {
        let selected_name_delete = selected_name.clone();
        move |_| {
            if !selected_name_delete.is_empty() && selected_name_delete != "Nenhum selecionado" {
                if fs::remove_file(&selected_name_delete).is_ok() {
                    reports.set(read_reports(Path::new("relatorios")));
                    on_select.call(String::from("Nenhum selecionado"));
                }
            }
        }
    };

    rsx! {
        div { class: "app-container",
            StatusModal {
                show: show_status_modal,
                status: status_modal_type(),
                message: status_msg(),
                sql_content: last_sql(),
                on_close: move |_| show_status_modal.set(false)
            }
            LogsModal { show: show_logs, on_close: move |_| show_logs.set(false) }
            ParamsModal { show: show_params_modal, report_config: current_report_config, report_path: current_report_path, on_close: move |_| show_params_modal.set(false), on_generate: execute_report }

            div { class: "middle-section",
                div { class: "sidebar",
                    button { class: "btn-classic", onclick: move |_| { on_select.call(String::from("relatorios/novo_relatorio.json")); on_edit.call(()); }, "✚ Novo" }
                    button { class: "btn-classic", onclick: move |_| on_edit.call(()), "✎ Editar" }
                    button { class: "btn-classic btn-danger", onclick: delete_selected, "✖ Excluir" }
                    div { class: "sidebar-spacer" }
                    button { class: "btn-classic btn-dark", onclick: move |_| show_logs.set(true), "📜 Logs" }
                }

                div { class: "main-view",
                    div { class: "top-toolbar",
                        input { class: "input-classic search-input", placeholder: "Pesquisa...", value: "{search_text}", oninput: move |evt| search_text.set(evt.value()) }
                        span { "Total: {total_files}" }
                        input { class: "input-classic selected-report-display", readonly: true, value: "{selected_name}" }
                    }
                    div { class: "tree-container",
                        ul { class: "tree-list",
                            {render_tree(&filtered_items, &selected_name, on_select, EventHandler::new(prepare_report))}
                        }
                    }
                }
            }

            div { class: "status-bar-container",
                span { "Relatório atual: {selected_name}" }
                if is_loading() {
                    div { class: "loading-group-minimal",
                        span { "{*progress.read() as i32}%" }
                        div { class: "progress-bar-mini-bg", div { class: "progress-bar-mini-fill", style: "width: {progress.read()}%" } }
                        button { class: "btn-abort-link", onclick: move |_| { cancel_flag.read().store(true, Ordering::SeqCst); is_loading.set(false); progress.set(0.0); }, "Cancelar" }
                    }
                }
            }
        }
    }
}
