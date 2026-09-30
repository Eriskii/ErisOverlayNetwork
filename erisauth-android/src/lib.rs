//! ErisAuth inside the Android app, bound to `dev.eris.auth.Native`.
//!
//! One daemon per process, on one runtime. Every call answers a JSON string
//! (see [`erisauth::device::mobile`]); `answer` and `asked` block, so the app
//! calls them off its main thread.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use erisauth::device::mobile::Mobile;
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
    json!({"error": "ErisAuth has not started"}).to_string()
}

/// Start the daemon with its state in `data`, or answer for the one running:
/// `{"page"}` with the approval page's link, or `{"error"}`.
pub fn start(data: PathBuf, runtime_dir: PathBuf) -> String {
    let mut slot = MOBILE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(mobile) = slot.as_ref() {
        return json!({"page": mobile.link()}).to_string();
    }
    match runtime().block_on(Mobile::start(data, runtime_dir)) {
        Ok(mobile) => {
            let answer = json!({"page": mobile.link()}).to_string();
            *slot = Some(mobile);
            answer
        }
        Err(e) => json!({"error": format!("{e:#}")}).to_string(),
    }
}

pub fn ask(ask: &str, caller: &str) -> String {
    match mobile() {
        Some(mobile) => runtime().block_on(mobile.ask(ask, caller)),
        None => not_started(),
    }
}

pub fn answer(id: &str) -> String {
    match mobile() {
        Some(mobile) => runtime().block_on(mobile.answer(id)),
        None => not_started(),
    }
}

pub fn asked(id: &str, within_ms: u64) -> String {
    match mobile() {
        Some(mobile) => runtime().block_on(mobile.asked(id, Duration::from_millis(within_ms))),
        None => not_started(),
    }
}

pub fn withdraw(id: &str) {
    if let Some(mobile) = mobile() {
        runtime().block_on(mobile.withdraw(id));
    }
}

#[cfg(target_os = "android")]
mod jni_bindings {
    use jni::JNIEnv;
    use jni::objects::{JClass, JString};
    use jni::sys::{jlong, jstring};

    fn text(env: &mut JNIEnv, s: &JString) -> String {
        env.get_string(s).map(Into::into).unwrap_or_default()
    }

    /// A panic must not unwind into the JVM, which aborts the app; it comes
    /// back as an error like any other.
    fn guarded(f: impl FnOnce() -> String + std::panic::UnwindSafe) -> String {
        std::panic::catch_unwind(f).unwrap_or_else(|_| serde_json::json!({"error": "ErisAuth panicked"}).to_string())
    }

    fn reply(env: &mut JNIEnv, answer: String) -> jstring {
        env.new_string(answer).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_dev_eris_auth_Native_start(
        mut env: JNIEnv,
        _class: JClass,
        data: JString,
        runtime: JString,
    ) -> jstring {
        let (data, runtime) = (text(&mut env, &data), text(&mut env, &runtime));
        let answer = guarded(move || super::start(data.into(), runtime.into()));
        reply(&mut env, answer)
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_dev_eris_auth_Native_ask(
        mut env: JNIEnv,
        _class: JClass,
        ask: JString,
        caller: JString,
    ) -> jstring {
        let (ask, caller) = (text(&mut env, &ask), text(&mut env, &caller));
        let answer = guarded(move || super::ask(&ask, &caller));
        reply(&mut env, answer)
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_dev_eris_auth_Native_answer(mut env: JNIEnv, _class: JClass, id: JString) -> jstring {
        let id = text(&mut env, &id);
        let answer = guarded(move || super::answer(&id));
        reply(&mut env, answer)
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_dev_eris_auth_Native_asked(
        mut env: JNIEnv,
        _class: JClass,
        id: JString,
        within_ms: jlong,
    ) -> jstring {
        let id = text(&mut env, &id);
        let answer = guarded(move || super::asked(&id, within_ms.max(0) as u64));
        reply(&mut env, answer)
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_dev_eris_auth_Native_withdraw(mut env: JNIEnv, _class: JClass, id: JString) {
        let id = text(&mut env, &id);
        let _ = std::panic::catch_unwind(move || super::withdraw(&id));
    }
}
