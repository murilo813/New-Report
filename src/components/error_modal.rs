use dioxus::prelude::*;

#[component]
pub fn SqlErrorModal(
    show: Signal<bool>,
    error_message: String,
    sql_content: String,
    on_close: EventHandler<()>,
) -> Element {
    if !show() {
        return rsx! {};
    }

    let sql_lines: Vec<&str> = sql_content.lines().collect();

    let highlight_term = if let Some(start) = error_message.find('\'') {
        if let Some(end) = error_message[start + 1..].find('\'') {
            Some(error_message[start + 1..start + 1 + end].to_string())
        } else {
            None
        }
    } else if error_message.contains("no such column:") {
        error_message
            .split("no such column: ")
            .nth(1)
            .and_then(|s| s.split_whitespace().next())
            .map(|s| s.to_string())
    } else {
        None
    };

    rsx! {
      div { class: "modal-overlay",
        div { class: "modal-window error-theme",
          div { class: "modal-header error-header",
            span { "⚠️ Alerta do Sistema" }
          }

          div { class: "modal-body",
            if !sql_content.is_empty() {
              div { class: "sql-error-viewer",
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

            div { class: "error-message-box",
              strong { "Informação:" }
              p { "{error_message}" }
            }
          }

          div { class: "modal-footer",
            button {
              class: "btn-classic btn-error",
              onclick: move |_| on_close.call(()),
              "Entendido"
            }
          }
        }
      }
    }
}
