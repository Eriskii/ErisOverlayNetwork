//! Telling the person something is waiting, through the desktop's
//! notification service. Choosing the notification opens the approval page.

use super::agent::Approval;

/// Show that `approval` is waiting. A desktop without a notification
/// service still has the page and the prompt; failing to notify is only logged.
pub fn asked(approval: &Approval, page: Option<String>) {
    let summary = format!("{} asks to sign in", approval.client);
    let program =
        approval.executable.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "an unknown program".into());
    let body = format!("{program} wants {}.", approval.requested.join(", "));
    tokio::task::spawn_blocking(move || {
        let mut notification = notify_rust::Notification::new();
        notification.appname("ErisAuth").summary(&summary).body(&body);
        if page.is_some() {
            notification.action("default", "Review");
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
