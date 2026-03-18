use crate::components::status_modal::{StatusModal, StatusType};
use crate::core::engine::{DataEngine, append_log};
use dioxus::prelude::*;

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

    let report_data = use_resource(move || {
        let current_engine = engine.clone();
        let sql = sql_to_query.clone();

        async move {
            let start_time = std::time::Instant::now();

            let engine_handle = current_engine.read();

            let res = match engine_handle.execute_user_sql(&sql, "Tela de Visualização") {
                Ok((cols, rows_vec)) => (cols, rows_vec),
                Err(e) => (vec!["ERRO".to_string()], vec![vec![e]]),
            };

            let elapsed_ms = start_time.elapsed().as_millis();
            append_log(
                "Visualização do Relatório",
                "Execução da Query DATAFUSION e Renderização",
                elapsed_ms,
            );

            res
        }
    });

    let (headers, all_rows) = match &*report_data.read() {
        Some((h, r)) => (h.clone(), r.clone()),
        None => (vec!["Processando...".to_string()], vec![]),
    };

    let total_rows = all_rows.len();

    let headers_export = headers.clone();
    let rows_export = all_rows.clone();

    let export_csv = move |_| {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Salvar Relatório CSV")
            .add_filter("Planilha CSV", &["csv"])
            .save_file()
        {
            let mut file_content = String::new();
            file_content.push_str(&headers_export.join(";"));
            file_content.push('\n');

            for row in &rows_export {
                file_content.push_str(&row.join(";"));
                file_content.push('\n');
            }

            if let Ok(_) = std::fs::write(&path, file_content) {
                status_msg.set(format!(
                    "Relatório exportado com sucesso para:\n{}",
                    path.display()
                ));
                status_modal_type.set(StatusType::Success);
                show_status_modal.set(true);
            } else {
                status_msg.set("Erro ao escrever arquivo CSV.".to_string());
                status_modal_type.set(StatusType::Error);
                show_status_modal.set(true);
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

    let status_text = if report_data.read().is_none() {
        "Processando Consulta SQL no Motor DataFusion...".to_string()
    } else if total_rows == 0 {
        "Consulta concluída: Nenhum registro encontrado para os filtros selecionados.".to_string()
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
                    div { class: "data-container",
                        if report_data.read().is_none() {
                            div { class: "empty-msg", "Analisando dados, por favor aguarde..." }
                        } else if total_rows == 0 {
                             div { class: "empty-msg", "Sem resultados. Revise os parâmetros da consulta." }
                        } else {
                            table { class: "pg-table table-wrapper",
                                thead {
                                    tr {
                                        for h in headers.iter() {
                                            th {
                                                key: "{h}",
                                                class: "sticky-header",
                                                "{h}"
                                            }
                                        }
                                    }
                                }
                                tbody {
                                    for (i, row) in visible_rows.iter().enumerate() {
                                        tr {
                                            key: "{i}",
                                            for (j, cell) in row.iter().enumerate() {
                                                td { key: "{j}", "{cell}" }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        if displayed_count > 0 && displayed_count < total_rows {
                            div {
                                class: "load-more-btn",
                                onclick: move |_| {
                                    max_visible.set(max_visible() + 1000);
                                },
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
