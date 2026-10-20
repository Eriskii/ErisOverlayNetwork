//! Telling the person through the desktop's notification service: a request waiting for them,
//! or a change another device made. Choosing a notification opens the approval page. A desktop
//! without a notification service still has the page and the terminal; failing to notify is
//! only logged. On Android the app posts its own notifications.

use super::Event;
use super::agent::{Kind, Prompt};

/// What a waiting request says in a notification: a summary and a body.
pub fn describe(prompt: &Prompt) -> (String, String) {
    match prompt.kind {
        Kind::Device => (
            format!("{} asks to join your devices", prompt.name),
            format!("Compare the code {} with the new device's screen.", prompt.fingerprint.as_deref().unwrap_or("?")),
        ),
        Kind::App => {
            let service = prompt.service.as_deref().unwrap_or("a service");
            let summary = if prompt.becomes_erisdb {
                format!("{} asks to be your ErisDB", prompt.name)
            } else {
                format!("{} asks to use {}", prompt.name, if service == "erisdb" { "ErisDB" } else { service })
            };
            let who = prompt.program.clone().unwrap_or_else(|| "An app with no program on this device".into());
            let wants = if prompt.grants.is_empty() { "nothing more".into() } else { prompt.grants.join(", ") };
            (summary, format!("{who} wants {wants}."))
        }
    }
}

/// Show that `prompt` waits for the person.
pub fn asked(prompt: &Prompt, page: Option<String>) {
    let (summary, body) = describe(prompt);
    show(summary, body, page);
}

/// Show that something changed.
pub fn event(event: &Event, page: Option<String>) {
    show("ErisAuth".into(), event.text.clone(), page);
}

/// On macOS, through Notification Center. A notification there cannot carry an action, so the
/// page opens from `erisauth open`.
#[cfg(target_os = "macos")]
fn show(summary: String, body: String, _page: Option<String>) {
    let quote = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    let script = format!("display notification {} with title \"ErisAuth\" subtitle {}", quote(&body), quote(&summary));
    if let Err(e) = std::process::Command::new("osascript").args(["-e", &script]).spawn() {
        eprintln!("erisauth: could not show a notification: {e}");
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn show(_summary: String, _body: String, _page: Option<String>) {}

#[cfg(target_os = "linux")]
fn show(summary: String, body: String, page: Option<String>) {
    tokio::task::spawn_blocking(move || {
        let mut notification = notify_rust::Notification::new();
        notification.appname("ErisAuth").summary(&summary).body(&body);
        if page.is_some() {
            notification.action("default", "Open");
        }
        match notification.show() {
            Ok(handle) => {
                if let Some(link) = page {
                    handle.wait_for_action(|action| {
                        if action == "default" {
                            super::open(&link);
                        }
                    });
                }
            }
            Err(e) => eprintln!("erisauth: could not show a notification: {e}"),
        }
    });
}
