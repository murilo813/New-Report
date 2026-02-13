use crate::components::error_modal::SqlErrorModal;
use crate::core::engine::DataEngine;
use dioxus::prelude::*;
use rusqlite::types::ValueRef;

#[component]
pub fn ViewReport(
    on_back: EventHandler<MouseEvent>,
    engine: Signal<DataEngine>,
    query_sql: String,
) -> Element {
    let mut show_error = use_signal(|| false);
    let error_msg = use_signal(|| String::new());

    let sql_limpo = query_sql
        .lines()
        .filter(|line| !line.trim().to_uppercase().contains("[SYNC:"))
        .collect::<Vec<_>>()
        .join("\n");

    let sql_to_query = sql_limpo.clone();

    let report_data = use_memo(move || {
        let engine_handle = engine.read();
        let conn = &engine_handle.sqlite;

        match conn.prepare(&sql_to_query) {
            Ok(mut stmt) => {
                let cols: Vec<String> = stmt
                    .column_names()
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect();

                let col_count = stmt.column_count();

                let rows_iter = stmt
                    .query_map([], |row| {
                        let mut row_data = Vec::new();
                        for i in 0..col_count {
                            let val = row.get_ref(i).unwrap();
                            row_data.push(match val {
                                ValueRef::Null => "".to_string(),
                                ValueRef::Integer(i) => i.to_string(),
                                ValueRef::Real(f) => format!("{:.2}", f),
                                ValueRef::Text(t) => String::from_utf8_lossy(t).to_string(),
                                _ => "[BIN]".to_string(),
                            });
                        }
                        Ok(row_data)
                    })
                    .expect("Erro ao mapear linhas do SQLite");

                let rows_vec: Vec<Vec<String>> = rows_iter.filter_map(|r| r.ok()).collect();
                (cols, rows_vec)
            }
            Err(_) => (Vec::new(), Vec::new()),
        }
    });

    let (headers, rows) = report_data.read().clone();

    rsx! {
        div { class: "app-container",
            SqlErrorModal {
                show: show_error,
                error_message: error_msg(),
                sql_content: query_sql,
                on_close: move |_| show_error.set(false)
            }

            div { class: "middle-section",
                div { class: "sidebar",
                    button {
                        class: "btn-classic",
                        onclick: move |evt| on_back.call(evt),
                        "🏠 Voltar"
                    }
                }

                div { class: "main-view report-view-container",
                    div { class: "top-toolbar",
                        span { class: "folder-name", "Visualização de Dados" }
                    }

                    div { class: "data-container",
                        table { class: "pg-table table-wrapper",
                            thead {
                                tr {
                                    {headers.iter().map(|h| rsx! {
                                        th { key: "{h}", class: "sticky-header", "{h}" }
                                    })}
                                }
                            }
                            tbody {
                                {rows.iter().enumerate().map(|(i, row)| rsx! {
                                    tr { key: "{i}",
                                        {row.iter().enumerate().map(|(j, cell)| rsx! {
                                            td { key: "{j}", "{cell}" }
                                        })}
                                    }
                                })}
                            }
                        }
                    }
                }
            }
            div { class: "status-bar-container",
                span { "Total de registros: {rows.len()}" }
            }
        }
    }
}
