use crate::components::status_modal::{StatusModal, StatusType};
use crate::core::engine::{DataEngine, append_log};
use dioxus::prelude::*;
use rusqlite::types::ValueRef;

#[component]
pub fn ViewReport(
    on_back: EventHandler<MouseEvent>,
    engine: Signal<DataEngine>,
    query_sql: String,
) -> Element {
    let mut show_status_modal = use_signal(|| false);
    let mut status_modal_type = use_signal(|| StatusType::Error);
    let mut status_msg = use_signal(|| String::new());

    let mut max_visible = use_signal(|| 1000);

    let sql_to_query = query_sql.clone();

    let report_data = use_signal(move || {
        let engine_handle = engine.read();

        let start_time = std::time::Instant::now();

        let res = match engine_handle.execute_user_sql(&sql_to_query) {
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
            Err(e) => (vec!["ERRO".to_string()], vec![vec![e]]),
        };

        let elapsed_ms = start_time.elapsed().as_millis();
        append_log(
            "Visualização do Relatório",
            "Execução da Query SQLite e Renderização",
            elapsed_ms,
        );

        res
    });

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

                    status_msg.set(format!(
                        "Relatório exportado com sucesso para:\n{}",
                        path.display()
                    ));
                    status_modal_type.set(StatusType::Success);
                    show_status_modal.set(true);
                }
                Err(e) => {
                    status_msg.set(format!("Erro ao exportar arquivo: {}", e));
                    status_modal_type.set(StatusType::Error);
                    show_status_modal.set(true);
                }
            }
        }
    };

    let end_idx = max_visible().min(total_rows);
    let visible_rows = if total_rows > 0 {
        &all_rows[0..end_idx]
    } else {
        &[]
    };
    let displayed_count = visible_rows.len();

    let status_text = if total_rows == 0 {
        "Nenhum registro encontrado".to_string()
    } else {
        format!("Exibindo {} de {} registros", displayed_count, total_rows)
    };

    rsx! {
        div { class: "app-container",
            StatusModal {
                show: show_status_modal,
                status: status_modal_type(),
                message: status_msg(),
                sql_content: query_sql.clone(),
                on_close: move |_| show_status_modal.set(false)
            }

            div { class: "middle-section",
                div { class: "sidebar",
                    button { class: "btn-classic", onclick: move |evt| on_back.call(evt), "🏠 Voltar" }
                    button { class: "btn-classic", onclick: export_csv, "💾 Exportar CSV" }
                }

                div { class: "main-view report-view-container",
                    div { class: "top-toolbar", span { class: "folder-name", "Visualização de Dados" } }
                    div { class: "data-container",
                        table { class: "pg-table table-wrapper",
                            thead {
                                tr { {headers.iter().map(|h| rsx! { th { key: "{h}", class: "sticky-header", "{h}" } })} }
                            }
                            tbody {
                                {visible_rows.iter().enumerate().map(|(i, row)| rsx! {
                                    tr { key: "{i}",
                                        {row.iter().enumerate().map(|(j, cell)| rsx! { td { key: "{j}", "{cell}" } })}
                                    }
                                })}
                            }
                        }

                        if displayed_count < total_rows {
                            div {
                                class: "load-more-btn",
                                onclick: move |_| { max_visible += 1000; },
                                "⬇️ Rolar para carregar mais registros..."
                            }
                        }
                    }
                }
            }
            div { class: "status-bar-container", span { "{status_text}" } }
        }
    }
}
