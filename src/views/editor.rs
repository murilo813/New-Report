use crate::components::status_modal::{StatusModal, StatusType};
use crate::core::engine::DataEngine;
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct ReportParameter {
    pub id: String,
    pub nome: String,
    pub tipo: String,
    pub valor_padrao: String,
    pub requerido: bool,
    #[serde(default)]
    pub extra: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
struct ReportData {
    descricao: String,
    query_sql: String,
    #[serde(default)]
    parametros: Vec<ReportParameter>,
}

#[derive(PartialEq, Clone, Copy)]
enum EditorTab {
    Info,
    Parametros,
    Sql,
}

// COMPONENTES
#[component]
fn InfoTab(
    report_pure_name: Signal<String>,
    report_folder: Signal<String>,
    description: Signal<String>,
    on_change_folder: EventHandler<()>,
) -> Element {
    rsx! {
        div { class: "info-tab-content",
            div { class: "form-group",
                label { "Nome do Arquivo:" }
                input { class: "input-classic", value: "{report_pure_name}", oninput: move |evt| report_pure_name.set(evt.value()) }
            }
            div { class: "form-group",
                label { "Localização:" }
                div { class: "folder-display folder-clickable", onclick: move |_| on_change_folder.call(()), span { "📂 {report_folder}" } }
            }
            div { class: "form-group",
                label { "Descrição:" }
                textarea { class: "desc-editor", value: "{description}", oninput: move |evt| description.set(evt.value()) }
            }
        }
    }
}

#[component]
fn ParametrosTab(
    parameters: Signal<Vec<ReportParameter>>,
    selected_param_idx: Signal<Option<usize>>,
) -> Element {
    let params_list = parameters.read().clone();
    let sel_idx = *selected_param_idx.read();

    rsx! {
        div { class: "params-container",
            div { class: "params-sidebar",
                div { class: "params-list",
                    if params_list.is_empty() {
                        div { class: "empty-msg", "Nenhum parâmetro criado." }
                    } else {
                        {params_list.iter().enumerate().map(|(i, p)| {
                            let is_selected = sel_idx == Some(i);
                            let item_class = if is_selected { "param-item selected" } else { "param-item" };
                            rsx! {
                                div { key: "{i}", class: "{item_class}", onclick: move |_| selected_param_idx.set(Some(i)), "[{p.id}] - {p.nome}" }
                            }
                        })}
                    }
                }
                div { class: "params-actions",
                    button { class: "btn-classic flex-1",
                        onclick: move |_| {
                            let mut p = parameters.write();
                            let new_idx = p.len();
                            p.push(ReportParameter {
                                id: format!("param_{}", new_idx + 1), nome: "Novo Parâmetro".to_string(),
                                tipo: "string".to_string(), valor_padrao: "".to_string(), requerido: true, extra: "".to_string()
                            });
                            selected_param_idx.set(Some(new_idx));
                        },
                        "➕ Novo"
                    }
                    button { class: "btn-classic flex-1 btn-delete-text",
                        onclick: move |_| {
                            if let Some(idx) = sel_idx {
                                parameters.write().remove(idx);
                                selected_param_idx.set(None);
                            }
                        },
                        "❌ Deletar"
                    }
                }
            }

            div { class: "param-editor",
                if let Some(idx) = sel_idx {
                    div { class: "info-tab-content no-padding",
                        div { class: "form-group",
                            label { "ID da Variável (No SQL use: [id_da_variavel])" }
                            input { class: "input-classic", value: "{parameters.read()[idx].id}", oninput: move |evt| { if let Some(i) = *selected_param_idx.read() { parameters.write()[i].id = evt.value(); } } }
                        }
                        div { class: "form-group",
                            label { "Nome de Exibição (Aparece para o usuário final)" }
                            input { class: "input-classic", value: "{parameters.read()[idx].nome}", oninput: move |evt| { if let Some(i) = *selected_param_idx.read() { parameters.write()[i].nome = evt.value(); } } }
                        }
                        div { class: "form-group",
                            label { "Tipo do Campo" }
                            select { class: "input-classic input-h28", value: "{parameters.read()[idx].tipo}",
                                onchange: move |evt| { if let Some(i) = *selected_param_idx.read() { parameters.write()[i].tipo = evt.value(); } },
                                option { value: "string", "Texto (String)" } option { value: "int", "Inteiro (Número Exato)" }
                                option { value: "float", "Decimal (Moeda / Quantidade)" } option { value: "data", "Data" } option { value: "pesquisa", "Pesquisa (Busca com SQL)" }
                            }
                        }
                        div { class: "form-group",
                            label { "Valor Padrão (Obrigatório para testar a query)" }
                            input { class: "input-classic", value: "{parameters.read()[idx].valor_padrao}", oninput: move |evt| { if let Some(i) = *selected_param_idx.read() { parameters.write()[i].valor_padrao = evt.value(); } } }
                        }
                        div { class: "form-group checkbox-group",
                            input { class: "checkbox-input", r#type: "checkbox", checked: "{parameters.read()[idx].requerido}",
                                onchange: move |evt| { if let Some(i) = *selected_param_idx.read() { parameters.write()[i].requerido = evt.checked(); } }
                            }
                            label { class: "checkbox-label", "Campo Obrigatório" }
                        }
                        div { class: "form-group mt-15",
                            label { "Parâmetros Extras (Ex: SQL de pesquisa com tag [SYNC: ...])" }
                            textarea { class: "input-classic extra-sql-area", value: "{parameters.read()[idx].extra}", oninput: move |evt| { if let Some(i) = *selected_param_idx.read() { parameters.write()[i].extra = evt.value(); } } }
                        }
                    }
                } else {
                    div { class: "empty-editor-msg", "Selecione um parâmetro na lista ao lado ou clique em '➕ Novo' para criar e editar suas propriedades." }
                }
            }
        }
    }
}

#[component]
fn SqlTab(query_text: Signal<String>) -> Element {
    rsx! {
        div { class: "sql-editor-container",
            div { class: "sql-instruction",
                span { "Defina as tabelas. Ex: " } code { "-- [SYNC: nfmestre(*), pessoas(id, nome)]" }
            }
            textarea { class: "sql-editor", spellcheck: false, value: "{query_text}", oninput: move |evt| query_text.set(evt.value()) }
        }
    }
}

// CONTROLLER
#[component]
pub fn EditQuery(report_name: String, on_back: EventHandler<MouseEvent>) -> Element {
    let mut active_tab = use_signal(|| EditorTab::Info);
    let mut query_text = use_signal(|| String::new());
    let mut description = use_signal(|| String::new());
    let mut parameters = use_signal(|| Vec::<ReportParameter>::new());
    let selected_param_idx = use_signal(|| None::<usize>);

    let report_pure_name = use_signal(|| {
        Path::new(&report_name)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    });
    let mut report_folder = use_signal(|| {
        Path::new(&report_name)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "relatorios".to_string())
    });

    let mut status_msg = use_signal(|| String::new());
    let mut show_status_modal = use_signal(|| false);
    let mut status_modal_type = use_signal(|| StatusType::Error);

    let report_name_for_load = report_name.clone();
    use_effect(move || {
        let path = Path::new(&report_name_for_load);
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&report_name_for_load) {
                if let Ok(data) = serde_json::from_str::<ReportData>(&content) {
                    query_text.set(data.query_sql);
                    description.set(data.descricao);
                    parameters.set(data.parametros);
                }
            }
        }
    });

    let get_final_path = move || -> PathBuf {
        let mut p = PathBuf::from(report_folder.read().clone());
        p.push(format!("{}.json", report_pure_name.read().clone()));
        p
    };

    let change_folder = move |_| {
        let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let initial_dir = current_dir.join(report_folder.read().clone());
        let dialog = rfd::FileDialog::new().set_directory(&initial_dir);
        if let Some(new_folder) = dialog.pick_folder() {
            let folder_str = if let Ok(rel) = new_folder.strip_prefix(&current_dir) {
                rel.to_string_lossy().to_string()
            } else {
                new_folder.to_string_lossy().to_string()
            };
            report_folder.set(folder_str);
        }
    };

    let handle_test = move |_| {
        let sql = query_text.read().clone();

        if !sql.to_uppercase().contains("[SYNC:") {
            status_msg.set("ERRO: Tag [SYNC: ...] não encontrada na query.".to_string());
            status_modal_type.set(StatusType::Error);
            show_status_modal.set(true);
            return;
        }

        let engine = DataEngine::new();
        let conn = &engine.sqlite;
        let re_header = regex::Regex::new(r"(?i)\[SYNC:\s*(?s)(.*?)\]").unwrap();
        let re_table = regex::Regex::new(r"([a-zA-Z0-9_]+)\s*\((.*?)\)").unwrap();

        let mut tables_to_mock = Vec::new();
        if let Some(content) = re_header.captures(&sql).and_then(|caps| caps.get(1)) {
            for cap in re_table.captures_iter(content.as_str()) {
                tables_to_mock.push(cap[1].to_string().to_lowercase());
            }
        }

        if tables_to_mock.is_empty() {
            status_msg.set(
                "Tag SYNC encontrada, mas nenhuma tabela foi declarada corretamente.".to_string(),
            );
            status_modal_type.set(StatusType::Error);
            show_status_modal.set(true);
            return;
        }

        for table_name in tables_to_mock {
            match engine
                .schema
                .iter()
                .find(|(k, _)| k.to_lowercase() == table_name)
            {
                Some((_, config)) => {
                    let col_defs: Vec<String> = config
                        .columns
                        .iter()
                        .map(|col| {
                            let dtype = match col.field_type.as_str() {
                                "I" => "INTEGER",
                                "F" => "REAL",
                                _ => "TEXT",
                            };
                            format!("\"{}\" {}", col.name, dtype)
                        })
                        .collect();
                    let _ = conn.execute(
                        &format!("CREATE TABLE {} ({})", table_name, col_defs.join(", ")),
                        [],
                    );
                }
                None => {
                    status_msg.set(format!(
                        "Tabela '{}' não existe no arquivo schema.toml!",
                        table_name
                    ));
                    status_modal_type.set(StatusType::Error);
                    show_status_modal.set(true);
                    return;
                }
            }
        }

        let mut clean_sql = re_header.replace_all(&sql, "").to_string();
        let re_vars = regex::Regex::new(r"\[([a-zA-Z0-9_]+)\]").unwrap();

        for cap in re_vars.captures_iter(&clean_sql.clone()) {
            let var_id = cap[1].to_string();
            if let Some(param) = parameters.read().iter().find(|p| p.id == var_id) {
                if param.valor_padrao.trim().is_empty() {
                    status_msg.set(format!("Erro de Validação: A variável '[{}]' está no SQL, mas o 'Valor Padrão' dela está vazio na aba de Parâmetros.", var_id));
                    status_modal_type.set(StatusType::Error);
                    show_status_modal.set(true);
                    return;
                }
                clean_sql = clean_sql.replace(&format!("[{}]", var_id), &param.valor_padrao);
            } else {
                status_msg.set(format!(
                    "Variável '[{}]' foi escrita no SQL, mas não foi criada na aba de Parâmetros!",
                    var_id
                ));
                status_modal_type.set(StatusType::Error);
                show_status_modal.set(true);
                return;
            }
        }

        let commands: Vec<&str> = clean_sql
            .split(';')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if commands.is_empty() {
            status_msg.set("O SQL está vazio após a tag de sincronização.".to_string());
            status_modal_type.set(StatusType::Error);
            show_status_modal.set(true);
            return;
        }

        for cmd in commands {
            if let Err(e) = conn.prepare(cmd) {
                status_msg.set(format!("Erro de Sintaxe no SQL:\n{}", e));
                status_modal_type.set(StatusType::Error);
                show_status_modal.set(true);
                return;
            }
        }

        status_msg.set("SQL Validado com Sucesso!\n\nAs variáveis foram identificadas e o valor padrão passou no teste.".to_string());
        status_modal_type.set(StatusType::Success);
        show_status_modal.set(true);
    };

    let report_name_original = report_name.clone();
    let on_back_action = on_back.clone();

    let save_and_exit = move |e: MouseEvent| {
        let sql = query_text.read().clone();
        let name = report_pure_name.read().clone();
        let path_to_save = get_final_path();

        if name.trim().is_empty() {
            status_msg.set("Nome do relatório inválido.".to_string());
            status_modal_type.set(StatusType::Error);
            show_status_modal.set(true);
            return;
        }
        if sql.trim().is_empty() {
            status_msg.set("O SQL não pode estar vazio.".to_string());
            status_modal_type.set(StatusType::Error);
            show_status_modal.set(true);
            return;
        }
        if !sql.to_uppercase().contains("[SYNC:") {
            status_msg.set("ERRO: É obrigatório definir as tabelas de origem na 1ª linha.\n\nExemplo:\n-- [SYNC: nfmestre(*)]".to_string());
            status_modal_type.set(StatusType::Error);
            show_status_modal.set(true);
            return;
        }

        let data = ReportData {
            descricao: description.read().clone(),
            query_sql: sql,
            parametros: parameters.read().clone(),
        };

        if let Ok(json_content) = serde_json::to_string_pretty(&data) {
            if fs::write(&path_to_save, json_content).is_ok() {
                let old_path = Path::new(&report_name_original);
                if path_to_save != old_path && old_path.exists() {
                    let _ = fs::remove_file(old_path);
                }
                on_back_action.call(e);
            }
        }
    };

    let active_tab_val = *active_tab.read();

    rsx! {
        div { class: "app-container",
            StatusModal {
                show: show_status_modal,
                status: status_modal_type(),
                message: status_msg(),
                sql_content: if status_msg().contains("SYNC") { query_text() } else { String::new() },
                on_close: move |_| show_status_modal.set(false)
            }

            div { class: "middle-section",
                div { class: "sidebar",
                    button { class: "btn-classic", onclick: save_and_exit, "Salvar e Sair" }
                    button { class: "btn-classic", onclick: move |e| on_back.call(e), "Cancelar" }
                    button { class: "btn-classic", onclick: handle_test, "Testar Query" }
                }

                div { class: "main-view",
                    div { class: "tabs-header",
                        div { class: if active_tab_val == EditorTab::Info { "tab-item active" } else { "tab-item" }, onclick: move |_| active_tab.set(EditorTab::Info), "ℹ️ Detalhes" }
                        div { class: if active_tab_val == EditorTab::Parametros { "tab-item active" } else { "tab-item" }, onclick: move |_| active_tab.set(EditorTab::Parametros), "⚙️ Parâmetros" }
                        div { class: if active_tab_val == EditorTab::Sql { "tab-item active" } else { "tab-item" }, onclick: move |_| active_tab.set(EditorTab::Sql), "📝 SQL & Sincronia" }
                    }

                    div { class: "data-container editor-main-container",
                        if active_tab_val == EditorTab::Info {
                            InfoTab { report_pure_name: report_pure_name, report_folder: report_folder, description: description, on_change_folder: EventHandler::new(change_folder) }
                        } else if active_tab_val == EditorTab::Parametros {
                            ParametrosTab { parameters: parameters, selected_param_idx: selected_param_idx }
                        } else {
                            SqlTab { query_text: query_text }
                        }
                    }
                }
            }
        }
    }
}
