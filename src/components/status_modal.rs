use dioxus::prelude::*;

#[derive(Clone, Copy, PartialEq)]
pub enum StatusType {
    Success,
    Error,
}

#[component]
pub fn StatusModal(
    show: Signal<bool>,
    status: StatusType,
    message: String,
    sql_content: String,
    on_close: EventHandler<()>,
) -> Element {
    if !show() {
        return rsx! {};
    }

    let sql_lines: Vec<&str> = sql_content.lines().collect();

    let highlight_term = if status == StatusType::Error {
        if let Some(start) = message.find('\'') {
            if let Some(end) = message[start + 1..].find('\'') {
                Some(message[start + 1..start + 1 + end].to_string())
            } else {
                None
            }
        } else if message.contains("no such column:") {
            message
                .split("no such column: ")
                .nth(1)
                .and_then(|s| s.split_whitespace().next())
                .map(|s| s.to_string())
        } else {
            None
        }
    } else {
        None
    };

    let is_error = status == StatusType::Error;
    let header_class = if is_error {
        "error-header"
    } else {
        "success-header"
    };
    let btn_class = if is_error { "btn-error" } else { "btn-success" };
    let msg_box_class = if is_error {
        "error-message-box"
    } else {
        "success-message-box"
    };
    let title_icon = if is_error {
        "⚠️ Alerta do Sistema"
    } else {
        "✅ Operação Concluída"
    };

    rsx! {
        div { class: "modal-overlay",
            div { class: "modal-window",
                div { class: "modal-header {header_class}",
                    span { "{title_icon}" }
                }

                div { class: "modal-body",
                    if !sql_content.is_empty() {
                        div { class: "sql-viewer",
                            {sql_lines.iter().enumerate().map(|(i, line)| {
                                let is_suspect = if let Some(ref term) = highlight_term {
                                    line.contains(term)
                                } else {
                                    false
                                };

                                rsx! {
                                    div {
                                        key: "{i}",
                                        class: if is_suspect { "code-line suspect-line" } else { "code-line" },
                                        span { class: "line-number", "{i + 1}" }
                                        span { class: "line-content", "{line}" }
                                    }
                                }
                            })}
                        }
                    }

                    div { class: "{msg_box_class}",
                        strong { "Informação:" }
                        p { "{message}" }
                    }
                }

                div { class: "modal-footer",
                    button {
                        class: "btn-classic {btn_class}",
                        onclick: move |_| on_close.call(()),
                        "Entendido"
                    }
                }
            }
        }
    }
}
