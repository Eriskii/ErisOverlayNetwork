//! Telling the person something is waiting, through the desktop's
//! notification service.

use super::agent::Approval;

/// Show that `approval` is waiting. A desktop without a notification
/// service still has the prompt; failing to notify is only logged.
pub fn asked(approval: &Approval) {
    let summary = format!("{} asks to use ErisDB", approval.client);
    let program =
        approval.executable.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "an unknown program".into());
    let body = format!("{program} wants {}.\nAnswer with `erisauth approvals`.", approval.requested.join(", "));
    tokio::task::spawn_blocking(move || {
        if let Err(e) = notify_rust::Notification::new().appname("ErisAuth").summary(&summary).body(&body).show() {
            eprintln!("erisauth: could not show a notification: {e}");
        }
    });
}
