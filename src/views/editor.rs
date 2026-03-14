use crate::components::error_modal::SqlErrorModal;
use crate::core::engine::DataEngine;
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Serialize, Debug, Clone)]
struct ReportData {
    descricao: String,
    query_sql: String,
}

#[derive(PartialEq, Clone, Copy)]
enum EditorTab {
    Info,
    Sql,
}

#[component]
pub fn EditQuery(report_name: String, on_back: EventHandler<MouseEvent>) -> Element {
    let mut active_tab = use_signal(|| EditorTab::Sql);
    let mut query_text = use_signal(|| String::new());
    let mut description = use_signal(|| String::new());

    let mut report_pure_name = use_signal(|| {
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

    let mut error_msg = use_signal(|| String::new());
    let mut show_error = use_signal(|| false);
    let mut show_success = use_signal(|| false);

    let report_name_for_load = report_name.clone();
    use_effect(move || {
        let path = Path::new(&report_name_for_load);
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&report_name_for_load) {
                if let Ok(data) = serde_json::from_str::<ReportData>(&content) {
                    query_text.set(data.query_sql);
                    description.set(data.descricao);
                }
            }
        }
    });

    let get_final_path = move || -> PathBuf {
        let mut p = PathBuf::from(report_folder.read().clone());
        let name = report_pure_name.read().clone();
        p.push(format!("{}.json", name));
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
            error_msg.set("ERRO: Tag [SYNC: ...] não encontrada na query.".to_string());
            show_error.set(true);
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
            error_msg.set(
                "Tag SYNC encontrada, mas nenhuma tabela foi declarada corretamente.".to_string(),
            );
            show_error.set(true);
            return;
        }

        for table_name in tables_to_mock {
            match engine
                .schema
                .iter()
                .find(|(k, _)| k.to_lowercase() == table_name)
            {
                Some((_, config)) => {
                    let mut col_defs = Vec::new();
                    for col in &config.columns {
                        let dtype = match col.field_type.as_str() {
                            "I" => "INTEGER",
                            "F" => "REAL",
                            _ => "TEXT",
                        };
                        col_defs.push(format!("\"{}\" {}", col.name, dtype));
                    }
                    let create_sql =
                        format!("CREATE TABLE {} ({})", table_name, col_defs.join(", "));
                    let _ = conn.execute(&create_sql, []);
                }
                None => {
                    error_msg.set(format!(
                        "Tabela '{}' não existe no arquivo schema.toml!",
                        table_name
                    ));
                    show_error.set(true);
                    return;
                }
            }
        }

        let clean_sql = re_header.replace_all(&sql, "").to_string();
        let commands: Vec<&str> = clean_sql
            .split(';')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        if commands.is_empty() {
            error_msg.set("O SQL está vazio após a tag de sincronização.".to_string());
            show_error.set(true);
            return;
        }

        for cmd in commands {
            if let Err(e) = conn.prepare(cmd) {
                error_msg.set(format!("Erro de Sintaxe no SQL:\n{}", e));
                show_error.set(true);
                return;
            }
        }

        show_success.set(true);
    };

    let report_name_original = report_name.clone();
    let on_back_action = on_back.clone();

    let save_and_exit = move |e: MouseEvent| {
        let sql = query_text.read().clone();
        let name = report_pure_name.read().clone();
        let path_to_save = get_final_path();

        if name.trim().is_empty() {
            error_msg.set("Nome do relatório inválido.".to_string());
            show_error.set(true);
            return;
        }

        if sql.trim().is_empty() {
            error_msg.set("O SQL não pode estar vazio.".to_string());
            show_error.set(true);
            return;
        }

        if !sql.to_uppercase().contains("[SYNC:") {
            error_msg.set("ERRO: É obrigatório definir as tabelas de origem na 1ª linha.\n\nExemplo:\n-- [SYNC: nfmestre(*), pessoas(id, nome)]".to_string());
            show_error.set(true);
            return;
        }

        let data = ReportData {
            descricao: description.read().clone(),
            query_sql: sql,
        };

        if let Ok(json_content) = serde_json::to_string_pretty(&data) {
            if let Ok(_) = fs::write(&path_to_save, json_content) {
                let old_path = Path::new(&report_name_original);
                if path_to_save != old_path && old_path.exists() {
                    let _ = fs::remove_file(old_path);
                }
                on_back_action.call(e);
            }
        }
    };

    rsx! {
      div { class: "app-container",
        SqlErrorModal {
          show: show_error,
          error_message: error_msg(),
          sql_content: if error_msg().contains("SYNC") { query_text() } else { String::new() },
          on_close: move |_| show_error.set(false)
        }

        if show_success() {
          div { class: "modal-overlay",
            div { class: "modal-window",
              div { class: "modal-header", "Validação Concluída" } 
              div { class: "modal-body",
                div { class: "test-ok",
                  span { class: "success-icon", "✅" }
                  p { "SQL Validado com Sucesso!" }
                  p { style: "font-size: 0.9em; color: var(--text-muted);", "Sintaxe e tabelas do SYNC estão corretas." }
                }
              }
              div { class: "modal-footer",
                button { class: "btn-classic", onclick: move |_| show_success.set(false), "OK" }
              }
            }
          }
        }

        div { class: "middle-section",
          div { class: "sidebar",
            button { class: "btn-classic", onclick: save_and_exit, "Salvar e Sair" }
            button { class: "btn-classic", onclick: move |e| on_back.call(e), "Cancelar" }
            button { class: "btn-classic", onclick: handle_test, "Testar Query" } 
          }

          div { class: "main-view",
            div { class: "tabs-header",
              div {
                class: if active_tab() == EditorTab::Sql { "tab-item active" } else { "tab-item" },
                onclick: move |_| active_tab.set(EditorTab::Sql), "📝 SQL & Sincronia"
              }
              div {
                class: if active_tab() == EditorTab::Info { "tab-item active" } else { "tab-item" },
                onclick: move |_| active_tab.set(EditorTab::Info), "ℹ️ Detalhes"
              }
            }

            div { class: "data-container editor-main-container",
              if active_tab() == EditorTab::Info {
                div { class: "info-tab-content",
                  div { class: "form-group",
                    label { "Nome do Arquivo:" }
                    input { class: "input-classic", value: "{report_pure_name}", oninput: move |evt| report_pure_name.set(evt.value()) }
                  }
                  div { class: "form-group",
                    label { "Localização:" }
                    div { class: "folder-display folder-clickable", onclick: change_folder, span { "📂 {report_folder}" } }
                  }
                  div { class: "form-group",
                    label { "Descrição:" }
                    textarea { class: "desc-editor", value: "{description}", oninput: move |evt| description.set(evt.value()) }
                  }
                }
              } else {
                div { class: "sql-editor-container",
                   div { class: "sql-instruction",
                       span { "Defina as tabelas. Ex: " }
                       code { "-- [SYNC: nfmestre(*), pessoas(id, nome)]" }
                   }
                  textarea {
                    class: "sql-editor",
                    oninput: move |evt| query_text.set(evt.value()),
                    spellcheck: false,
                    "{query_text}"
                  }
                }
              }
            }
          }
        }
      }
    }
}
