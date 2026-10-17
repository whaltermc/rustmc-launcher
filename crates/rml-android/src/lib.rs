use jni::objects::JClass;
use jni::sys::jstring;
use jni::JNIEnv;

fn jstring(env: &mut JNIEnv, s: &str) -> jstring {
    env.new_string(s)
        .map(|o| o.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

#[no_mangle]
pub extern "system" fn Java_dev_rustmc_launcher_MainActivity_nativeInfo(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let msg = format!(
        "RustMC Launcher\ncore v{}\ntarget: {}\nstatus: not logged in",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::ARCH,
    );
    jstring(&mut env, &msg)
}

#[no_mangle]
pub extern "system" fn Java_dev_rustmc_launcher_MainActivity_nativeLogin(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    jstring(&mut env, "Device-flow login is not wired up yet.")
}
