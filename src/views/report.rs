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
    let mut error_msg = use_signal(|| String::new()); 

    let mut max_visible = use_signal(|| 1000);

    let sql_to_query = query_sql.clone();

    let report_data = use_memo(move || {
        let engine_handle = engine.read();

        match engine_handle.execute_user_sql(&sql_to_query) {
            Ok(mut stmt) => {
                let cols: Vec<String> = stmt
                    .column_names()
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect();

                let col_count = stmt.column_count();

                let rows_iter = stmt
                    .query_map([], |row| {
                        let mut row_data = Vec::with_capacity(col_count);
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
            Err(e) => {
                (vec!["ERRO".to_string()], vec![vec![e]])
            }
        }
    });

    // --- FUNÇÃO DE EXPORTAR CSV ---
    let (headers, all_rows) = report_data.read().clone();
    let total_rows = all_rows.len();
    
    let headers_export = headers.clone();
    let rows_export = all_rows.clone();

    let export_csv = move |_| {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Salvar Relatório CSV")
            .add_filter("Planilha CSV", &["csv"])
            .save_file()
        {
            match csv::WriterBuilder::new().delimiter(b';').from_path(&path) {
                Ok(mut wtr) => {
                    let _ = wtr.write_record(&headers_export);
                    for row in &rows_export {
                        let _ = wtr.write_record(row);
                    }
                    let _ = wtr.flush();
                    
                    error_msg.set(format!("Relatório exportado com sucesso para:\n{}", path.display()));
                    show_error.set(true);
                }
                Err(e) => {
                    error_msg.set(format!("Erro ao exportar arquivo: {}", e));
                    show_error.set(true);
                }
            }
        }
    };

    let end_idx = max_visible().min(total_rows);
    let visible_rows = if total_rows > 0 { &all_rows[0..end_idx] } else { &[] };
    let displayed_count = visible_rows.len();

    let status_text = if total_rows == 0 {
        "Nenhum registro encontrado".to_string()
    } else {
        format!("Exibindo {} de {} registros", displayed_count, total_rows)
    };

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
                    
                    button {
                        class: "btn-classic",
                        onclick: export_csv,
                        "💾 Exportar CSV"
                    }
                }

                div { class: "main-view report-view-container",
                    div { class: "top-toolbar",
                        span { class: "folder-name", "Visualização de Dados" }
                    }

                    div { class: "data-container", style: "overflow-y: auto;",
                        table { class: "pg-table table-wrapper",
                            thead {
                                tr {
                                    {headers.iter().map(|h| rsx! {
                                        th { key: "{h}", class: "sticky-header", "{h}" }
                                    })}
                                }
                            }
                            tbody {
                                {visible_rows.iter().enumerate().map(|(i, row)| rsx! {
                                    tr { key: "{i}",
                                        {row.iter().enumerate().map(|(j, cell)| rsx! {
                                            td { key: "{j}", "{cell}" }
                                        })}
                                    }
                                })}
                            }
                        }
                        
                        if displayed_count < total_rows {
                            div {
                                style: "text-align: center; padding: 20px; background: var(--bg-color-light); cursor: pointer; font-weight: bold; border-top: 1px solid var(--border-color);",
                                onclick: move |_| {
                                    max_visible += 1000;
                                },
                                "⬇️ Rolar para carregar mais registros..."
                            }
                        }
                    }
                }
            }
            div { class: "status-bar-container",
                span { "{status_text}" }
            }
        }
    }
}