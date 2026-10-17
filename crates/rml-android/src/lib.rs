use jni::objects::{JClass, JString};
use jni::sys::jstring;
use jni::JNIEnv;
use rml_auth::{Account, AccountKind, MicrosoftAuth, PollOutcome};
use rml_runtime::RuntimeSpec;
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

#[derive(Clone)]
struct InstallState {
    state: &'static str,
    label: String,
    done: u64,
    total: u64,
    message: String,
}

static INSTALL: OnceLock<Mutex<InstallState>> = OnceLock::new();

fn install_state() -> &'static Mutex<InstallState> {
    INSTALL.get_or_init(|| {
        Mutex::new(InstallState {
            state: "idle",
            label: String::new(),
            done: 0,
            total: 0,
            message: String::new(),
        })
    })
}

#[derive(Clone)]
enum JvmState {
    Idle,
    Running(String),
}

static JVM: OnceLock<Mutex<JvmState>> = OnceLock::new();

fn jvm_state() -> &'static Mutex<JvmState> {
    JVM.get_or_init(|| Mutex::new(JvmState::Idle))
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

fn runtime_status_json() -> String {
    let views: Vec<serde_json::Value> = rml_runtime::installed()
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "name": r.name,
                "major": r.major,
                "javaVersion": r.java_version,
                "installed": r.is_complete(),
            })
        })
        .collect();
    serde_json::Value::Array(views).to_string()
}

#[no_mangle]
pub extern "system" fn Java_dev_rustmc_launcher_MainActivity_nativeRuntimeStatus(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let json = runtime_status_json();
    jstring(&mut env, &json)
}

#[no_mangle]
pub extern "system" fn Java_dev_rustmc_launcher_MainActivity_nativeRuntimeInstall(
    mut env: JNIEnv,
    _class: JClass,
    name: JString,
) -> jstring {
    let name: String = match env.get_string(&name) {
        Ok(s) => s.into(),
        Err(_) => return jstring(&mut env, "error: bad runtime name"),
    };
    let Some(spec): Option<&'static RuntimeSpec> = rml_runtime::spec(&name) else {
        return jstring(&mut env, &format!("error: unknown runtime '{name}'"));
    };
    if let Ok(mut g) = install_state().lock() {
        if g.state == "running" {
            return jstring(&mut env, "error: another install is running");
        }
        *g = InstallState {
            state: "running",
            label: "starting".into(),
            done: 0,
            total: 0,
            message: String::new(),
        };
    }
    std::thread::spawn(move || {
        let progress = |done: u64, total: u64, label: &str| {
            if let Ok(mut g) = install_state().lock() {
                g.state = "running";
                g.label = label.to_string();
                g.done = done;
                g.total = total;
            }
        };
        let result = rml_runtime::install(spec, &progress);
        if let Ok(mut g) = install_state().lock() {
            match result {
                Ok(dir) => {
                    g.state = "done";
                    g.label = "done".into();
                    g.message = dir.display().to_string();
                }
                Err(e) => {
                    g.state = "error";
                    g.message = format!("{e}");
                }
            }
        }
    });
    jstring(&mut env, "started")
}

#[no_mangle]
pub extern "system" fn Java_dev_rustmc_launcher_MainActivity_nativeRuntimeProgress(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let s = install_state()
        .lock()
        .map(|g| {
            format!(
                "{}:{}:{}:{}:{}",
                g.state, g.label, g.done, g.total, g.message
            )
        })
        .unwrap_or_else(|_| "error:lock:0:0:".to_string());
    jstring(&mut env, &s)
}

#[no_mangle]
pub extern "system" fn Java_dev_rustmc_launcher_MainActivity_nativeJvmProbe(
    mut env: JNIEnv,
    _class: JClass,
    name: JString,
) -> jstring {
    let name: String = match env.get_string(&name) {
        Ok(s) => s.into(),
        Err(_) => return jstring(&mut env, "error: bad runtime name"),
    };
    if let Ok(mut g) = jvm_state().lock() {
        match &*g {
            JvmState::Running(_) => return jstring(&mut env, "error: jvm probe already running"),
            _ => *g = JvmState::Running(name.clone()),
        }
    }
    std::thread::spawn(move || {
        let result = rml_jvm::probe(&name);
        if let Ok(mut g) = jvm_state().lock() {
            *g = match result {
                Ok(Some(v)) => JvmState::Running(format!("ok: {v}")),
                Ok(None) => JvmState::Running("ok: (no java.version)".into()),
                Err(e) => JvmState::Running(format!("error: {e}")),
            };
        }
    });
    jstring(&mut env, "started")
}

#[no_mangle]
pub extern "system" fn Java_dev_rustmc_launcher_MainActivity_nativeJvmProbeState(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let s = jvm_state()
        .lock()
        .map(|g| match &*g {
            JvmState::Idle => "idle".to_string(),
            JvmState::Running(v) => v.clone(),
        })
        .unwrap_or_else(|_| "error:lock".to_string());
    jstring(&mut env, &s)
}
