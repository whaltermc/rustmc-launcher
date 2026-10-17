use jni::objects::{JClass, JString};
use jni::sys::jstring;
use jni::JNIEnv;
use rml_auth::{Account, AccountKind, MicrosoftAuth, PollOutcome};
use std::sync::{Mutex, OnceLock};

#[derive(Clone)]
enum LoginState {
    Idle,
    Waiting(String),
    Ok(String),
    Err(String),
}

static LOGIN: OnceLock<Mutex<LoginState>> = OnceLock::new();

fn login() -> &'static Mutex<LoginState> {
    LOGIN.get_or_init(|| Mutex::new(LoginState::Idle))
}

fn set_login(state: LoginState) {
    if let Ok(mut g) = login().lock() {
        *g = state;
    }
}

fn jstring(env: &mut JNIEnv, s: &str) -> jstring {
    env.new_string(s)
        .map(|o| o.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// Compact JSON view of the stored accounts (no tokens leaked to the UI layer).
fn accounts_json() -> String {
    let views: Vec<serde_json::Value> = rml_auth::load_accounts()
        .into_iter()
        .map(|a| {
            serde_json::json!({
                "kind": match a.kind {
                    AccountKind::Microsoft => "microsoft",
                    AccountKind::Offline => "offline",
                },
                "username": a.username,
                "uuid": a.uuid,
            })
        })
        .collect();
    serde_json::Value::Array(views).to_string()
}

#[no_mangle]
pub extern "system" fn Java_dev_rustmc_launcher_MainActivity_nativeInfo(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let msg = format!(
        "RustMC Launcher\ncore v{}\ntarget: {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::ARCH,
    );
    jstring(&mut env, &msg)
}

#[no_mangle]
pub extern "system" fn Java_dev_rustmc_launcher_MainActivity_nativeAccounts(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let json = accounts_json();
    jstring(&mut env, &json)
}

#[no_mangle]
pub extern "system" fn Java_dev_rustmc_launcher_MainActivity_nativeAddOffline(
    mut env: JNIEnv,
    _class: JClass,
    name: JString,
) -> jstring {
    let name: String = match env.get_string(&name) {
        Ok(s) => s.into(),
        Err(_) => return jstring(&mut env, r#"{"error":"bad name"}"#),
    };
    let name = name.trim();
    if name.is_empty() {
        return jstring(&mut env, r#"{"error":"empty name"}"#);
    }
    let account = Account::offline(name);
    if let Err(e) = rml_auth::upsert_account(&account) {
        return jstring(&mut env, &format!(r#"{{"error":"{e}"}}"#));
    }
    let json = accounts_json();
    jstring(&mut env, &json)
}

#[no_mangle]
pub extern "system" fn Java_dev_rustmc_launcher_MainActivity_nativeLoginBegin(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let auth = MicrosoftAuth::new();
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => return jstring(&mut env, &format!("error: {e}")),
    };
    let device = match rt.block_on(auth.request_device_code()) {
        Ok(d) => d,
        Err(e) => {
            set_login(LoginState::Err(format!("{e}")));
            return jstring(&mut env, &format!("error: {e}"));
        }
    };
    let message = device.message.clone().unwrap_or_else(|| {
        format!(
            "Go to {} and enter code {}",
            device.verification_uri, device.user_code
        )
    });
    set_login(LoginState::Waiting(message.clone()));

    let code = device.device_code.clone();
    let interval = device.interval.max(5);
    let expires_in = device.expires_in.max(60) as u64;
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => return set_login(LoginState::Err(format!("{e}"))),
        };
        rt.block_on(async move {
            let start = std::time::Instant::now();
            loop {
                if start.elapsed().as_secs() > expires_in {
                    set_login(LoginState::Err("login timed out".into()));
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
                match auth.poll(&code).await {
                    Ok(PollOutcome::Success(a)) => {
                        let _ = rml_auth::upsert_account(&a);
                        set_login(LoginState::Ok(a.username));
                        return;
                    }
                    Ok(PollOutcome::Pending | PollOutcome::SlowDown) => {}
                    Err(e) => {
                        set_login(LoginState::Err(format!("{e}")));
                        return;
                    }
                }
            }
        });
    });

    jstring(&mut env, &message)
}

#[no_mangle]
pub extern "system" fn Java_dev_rustmc_launcher_MainActivity_nativeLoginState(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let state = login().lock().map(|g| g.clone()).unwrap_or(LoginState::Idle);
    let s = match state {
        LoginState::Idle => "idle".to_string(),
        LoginState::Waiting(m) => format!("waiting: {m}"),
        LoginState::Ok(u) => format!("ok: {u}"),
        LoginState::Err(e) => format!("error: {e}"),
    };
    jstring(&mut env, &s)
}
