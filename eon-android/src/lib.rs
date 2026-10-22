//! ErisOverlayNetwork inside the Android app, bound to `dev.eris.eon.Native`.
//!
//! One daemon per process, on one runtime, reaching the other devices over the internet. Every
//! call answers a JSON string (see [`eon::device::mobile`]); `command` and `request` block,
//! so the app calls them off its main thread.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use eon::device::mobile::Mobile;
use erisoverlaynetwork::net::Network;
use serde_json::json;

fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("a runtime"))
}

static MOBILE: Mutex<Option<Mobile>> = Mutex::new(None);

fn mobile() -> Option<Mobile> {
    MOBILE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn not_started() -> String {
    json!({"error": "not_started", "message": "ErisOverlayNetwork has not started"}).to_string()
}

/// Start the daemon with its key and mesh in `data`, unless it runs already: `{"ok": true}`, or
/// `{"error", "message"}`.
pub fn start(data: PathBuf) -> String {
    let mut slot = MOBILE.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_some() {
        return json!({"ok": true}).to_string();
    }
    match runtime().block_on(Mobile::start(data, Network::Internet)) {
        Ok(mobile) => {
            *slot = Some(mobile);
            json!({"ok": true}).to_string()
        }
        Err(e) => json!({"error": "not_started", "message": format!("{e:#}")}).to_string(),
    }
}

/// One command from the app's screens.
pub fn command(message: &str) -> String {
    match mobile() {
        Some(mobile) => runtime().block_on(mobile.command(message)),
        None => not_started(),
    }
}

/// An app's request line, from `caller`, under the service's `key` for it.
pub fn request(key: &str, line: &str, caller: &str) -> String {
    match mobile() {
        Some(mobile) => runtime().block_on(mobile.request(key, line, caller)),
        None => json!({"error": "ErisOverlayNetwork has not started"}).to_string(),
    }
}

/// Take back the request under `key`.
pub fn withdraw(key: &str) {
    if let Some(mobile) = mobile() {
        mobile.withdraw(key);
    }
}

#[cfg(target_os = "android")]
mod jni_bindings {
    use jni::JNIEnv;
    use jni::objects::{JClass, JString};
    use jni::sys::jstring;

    fn text(env: &mut JNIEnv, s: &JString) -> String {
        env.get_string(s).map(Into::into).unwrap_or_default()
    }

    /// A panic must not unwind into the JVM, which aborts the app; it comes back as an error
    /// like any other.
    fn guarded(f: impl FnOnce() -> String + std::panic::UnwindSafe) -> String {
        std::panic::catch_unwind(f).unwrap_or_else(|_| {
            serde_json::json!({"error": "internal", "message": "ErisOverlayNetwork panicked"}).to_string()
        })
    }

    fn reply(env: &mut JNIEnv, answer: String) -> jstring {
        env.new_string(answer).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_dev_eris_eon_Native_start(mut env: JNIEnv, _class: JClass, data: JString) -> jstring {
        let data = text(&mut env, &data);
        let answer = guarded(move || super::start(data.into()));
        reply(&mut env, answer)
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_dev_eris_eon_Native_command(
        mut env: JNIEnv,
        _class: JClass,
        message: JString,
    ) -> jstring {
        let message = text(&mut env, &message);
        let answer = guarded(move || super::command(&message));
        reply(&mut env, answer)
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_dev_eris_eon_Native_request(
        mut env: JNIEnv,
        _class: JClass,
        key: JString,
        line: JString,
        caller: JString,
    ) -> jstring {
        let (key, line, caller) = (text(&mut env, &key), text(&mut env, &line), text(&mut env, &caller));
        let answer = guarded(move || super::request(&key, &line, &caller));
        reply(&mut env, answer)
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_dev_eris_eon_Native_withdraw(mut env: JNIEnv, _class: JClass, key: JString) {
        let key = text(&mut env, &key);
        let _ = std::panic::catch_unwind(move || super::withdraw(&key));
    }
}
