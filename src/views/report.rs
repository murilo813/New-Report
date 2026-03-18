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

    let mut headers = use_signal(|| Vec::<String>::new());
    let mut visible_rows = use_signal(|| Vec::<Vec<String>>::new());
    let mut total_rows_count = use_signal(|| 0usize);
    let mut current_offset = use_signal(|| 0usize);

    let sql_to_query = query_sql.clone();

    let report_task = use_resource(move || {
        let engine_handle = engine;
        let sql = sql_to_query.clone();

        async move {
            let start_time = std::time::Instant::now();
            
            let res = engine_handle.read().execute_user_sql(&sql, "Tela de Visualização");

            if let Ok((cols, total)) = &res { 
                headers.set(cols.clone()); 
                total_rows_count.set(*total);
                
                let first_chunk = engine_handle.read().get_rows_slice(0, 200);
                visible_rows.set(first_chunk);
                current_offset.set(200);
            }

            append_log(
                "Visualização", 
                "Execução SQL e Carga Inicial 200", 
                start_time.elapsed().as_millis()
            );
            res
        }
    });


    let load_more = move |_| {
        let offset = current_offset();
        let next_chunk = engine.read().get_rows_slice(offset, 200);
        
        visible_rows.write().extend(next_chunk);
        current_offset.set(offset + 200);
    };

    let export_csv = move |_| {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Salvar Relatório CSV")
            .add_filter("Planilha CSV", &["csv"])
            .save_file() 
        {
            let all_data = engine.read().get_rows_slice(0, total_rows_count());
            let mut file_content = String::new();
            file_content.push_str(&headers.read().join(";"));
            file_content.push('\n');

            for row in all_data {
                file_content.push_str(&row.join(";"));
                file_content.push('\n');
            }

            if let Ok(_) = std::fs::write(&path, file_content) {
                status_msg.set(format!("Exportado com sucesso para:\n{}", path.display()));
                status_modal_type.set(StatusType::Success);
                show_status_modal.set(true);
            }
        }
    };

    let status_text = if report_task.read().is_none() {
        "Processando Consulta no Motor DataFusion...".to_string()
    } else {
        format!("Exibindo {} de {} registros", visible_rows.read().len(), total_rows_count())
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
                    div { class: "top-toolbar", span { class: "folder-name", "Visualização de Dados (Lazy Loading)" } }
                    
                    div { class: "data-container",
                        if report_task.read().is_none() {
                            div { class: "empty-msg", "Analisando dados, por favor aguarde..." }
                        } else if total_rows_count() == 0 {
                             div { class: "empty-msg", "Sem resultados para esta consulta." }
                        } else {
                            table { class: "pg-table table-wrapper",
                                thead {
                                    tr {
                                        for h in headers.read().iter() {
                                            th { key: "{h}", class: "sticky-header", "{h}" }
                                        }
                                    }
                                }
                                tbody {
                                    for (i, row) in visible_rows.read().iter().enumerate() {
                                        tr { key: "{i}",
                                            for (j, cell) in row.iter().enumerate() {
                                                td { key: "{j}", "{cell}" }
                                            }
                                        }
                                    }
                                }
                            }
                            
                            if current_offset() < total_rows_count() {
                                div { 
                                    class: "load-more-btn", 
                                    onclick: load_more,
                                    "⬇️ Mostrar mais 200 registros..."
                                }
                            }
                        }
                    }
                }
            }
            div { class: "status-bar-container", span { "{status_text}" } }
        }
    }
}