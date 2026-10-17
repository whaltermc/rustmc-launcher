//! In-process JVM bootstrap for Android.
//!
//! Android ships no `libjvm` in the system linker namespace and the prebuilt Mojang/Mojo
//! runtime tarballs carry no usable `DT_RUNPATH`, so we cannot link against `libjvm` at
//! compile time. Instead we `dlopen` the runtime's `lib/server/libjvm.so` and resolve
//! `JNI_CreateJavaVM` at runtime, then hand the raw pointer to the `jni` crate.
//!
//! Note: full Pojav/Mojo parity also needs the "ELF hinter" (rewriting `DT_RUNPATH` and
//! opening libraries through a private linker namespace) which is a follow-up; here we
//! best-effort preload the runtime's own shared objects with `RTLD_GLOBAL`.

use std::ffi::{c_char, c_int, c_void, CString};
use std::path::{Path, PathBuf};

const JNI_VERSION_1_8: c_int = 0x0001_0008;
const JNI_OK: c_int = 0;
const RTLD_NOW: c_int = 2;
const RTLD_GLOBAL: c_int = 0x100;

#[repr(C)]
struct JavaVMOption {
    option_string: *mut c_char,
    extra_info: *mut c_void,
}

#[repr(C)]
struct JavaVMInitArgs {
    version: c_int,
    n_options: c_int,
    options: *mut JavaVMOption,
    ignore_unrecognized: u8,
}

type CreateJavaVm = unsafe extern "system" fn(
    *mut *mut c_void,
    *mut *mut c_void,
    *mut JavaVMInitArgs,
) -> c_int;

extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlerror() -> *mut c_char;
}

fn last_dl_error() -> String {
    unsafe {
        let e = dlerror();
        if e.is_null() {
            "unknown dlopen error".to_string()
        } else {
            std::ffi::CStr::from_ptr(e).to_string_lossy().to_string()
        }
    }
}

fn dlopen_global(path: &Path) -> Result<*mut c_void, JvmError> {
    let c = CString::new(path.to_string_lossy().as_bytes())
        .map_err(|_| JvmError::InvalidPath(path.to_path_buf()))?;
    unsafe {
        let h = dlopen(c.as_ptr(), RTLD_NOW | RTLD_GLOBAL);
        if h.is_null() {
            return Err(JvmError::DlOpen {
                path: path.to_path_buf(),
                error: last_dl_error(),
            });
        }
        Ok(h)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum JvmError {
    #[error("invalid path: {0}")]
    InvalidPath(PathBuf),
    #[error("dlopen {path}: {error}")]
    DlOpen { path: PathBuf, error: String },
    #[error("symbol {0} not found in libjvm")]
    MissingSymbol(String),
    #[error("JNI_CreateJavaVM failed with code {0}")]
    CreateFailed(c_int),
    #[error("jni error: {0}")]
    Jni(#[from] jni::errors::Error),
    #[error("runtime not installed: {0}")]
    NoRuntime(String),
}

/// An initialized JVM plus the `dlopen` handle that must outlive it.
pub struct Jvm {
    vm: jni::JavaVM,
    _handle: *mut c_void,
}

impl Jvm {
    pub fn vm(&self) -> &jni::JavaVM {
        &self.vm
    }

    /// Run `java.lang.System.getProperty(key)` to prove the VM is alive.
    pub fn system_property(&self, key: &str) -> Result<Option<String>, JvmError> {
        let mut env = self.vm.attach_current_thread()?;
        let key = env.new_string(key)?;
        let value = env
            .call_static_method(
                "java/lang/System",
                "getProperty",
                "(Ljava/lang/String;)Ljava/lang/String;",
                &[(&key).into()],
            )?
            .l()?;
        if value.is_null() {
            return Ok(None);
        }
        let value: jni::objects::JString = value.into();
        let value: String = env.get_string(&value)?.into();
        Ok(Some(value))
    }
}

/// Preload shared objects that `libjvm` links against, with `RTLD_GLOBAL`, so that dlsym
/// and lazy symbol resolution inside `libjvm` can find them without a runpath.
fn preload(runtime_dir: &Path) -> Vec<PathBuf> {
    let mut loaded = Vec::new();
    let mut candidates = Vec::new();
    for sub in ["lib", "lib/server"] {
        let dir = runtime_dir.join(sub);
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().map(|x| x == "so").unwrap_or(false) {
                    candidates.push(p);
                }
            }
        }
    }
    // libjvm last so that its own unresolved symbols see the others first.
    candidates.sort_by_key(|p| p.file_name().map(|n| n == "libjvm.so").unwrap_or(false));
    for p in candidates {
        if p.file_name().map(|n| n == "libjvm.so").unwrap_or(false) {
            continue;
        }
        if dlopen_global(&p).is_ok() {
            loaded.push(p);
        }
    }
    loaded
}

/// Create a JVM from an explicit `libjvm.so` path and option strings
/// (each must be a valid `JavaVMOption`, e.g. `-Dfoo=bar`).
pub fn create_with_libjvm(libjvm: &Path, options: &[String]) -> Result<Jvm, JvmError> {
    let runtime_dir = libjvm
        .parent()
        .and_then(|p| p.parent())
        .map(Path::to_path_buf)
        .ok_or_else(|| JvmError::InvalidPath(libjvm.to_path_buf()))?;

    // Env hints for the system linker; the preload below does the real work on Android.
    let existing = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
    let lib = runtime_dir.join("lib");
    let server = lib.join("server");
    let joined = format!(
        "{}:{}:{}",
        server.display(),
        lib.display(),
        existing
    );
    std::env::set_var("LD_LIBRARY_PATH", joined);
    std::env::set_var("JAVA_HOME", &runtime_dir);

    let _preloaded = preload(&runtime_dir);

    let handle = dlopen_global(libjvm)?;
    let sym = unsafe { dlsym(handle, c"JNI_CreateJavaVM".as_ptr()) };
    if sym.is_null() {
        return Err(JvmError::MissingSymbol("JNI_CreateJavaVM".into()));
    }
    let create: CreateJavaVm = unsafe { std::mem::transmute(sym) };

    let c_options: Vec<CString> = options
        .iter()
        .map(|o| CString::new(o.as_str()).unwrap_or_default())
        .collect();
    let mut vm_options: Vec<JavaVMOption> = c_options
        .iter()
        .map(|o| JavaVMOption {
            option_string: o.as_ptr() as *mut c_char,
            extra_info: std::ptr::null_mut(),
        })
        .collect();

    let mut args = JavaVMInitArgs {
        version: JNI_VERSION_1_8,
        n_options: vm_options.len() as c_int,
        options: vm_options.as_mut_ptr(),
        ignore_unrecognized: 0,
    };
    let mut vm: *mut c_void = std::ptr::null_mut();
    let mut env: *mut c_void = std::ptr::null_mut();
    let rc = unsafe { create(&mut vm, &mut env, &mut args) };
    if rc != JNI_OK || vm.is_null() {
        return Err(JvmError::CreateFailed(rc));
    }
    let vm = unsafe { jni::JavaVM::from_raw(vm as *mut jni::sys::JavaVM)? };
    Ok(Jvm { vm, _handle: handle })
}

/// Create a JVM using an installed runtime by name.
pub fn create(runtime_name: &str, options: &[String]) -> Result<Jvm, JvmError> {
    let libjvm = rml_runtime::libjvm_path(runtime_name);
    if !libjvm.exists() {
        return Err(JvmError::NoRuntime(runtime_name.to_string()));
    }
    create_with_libjvm(&libjvm, options)
}

/// Convenience probe: create a JVM, read `java.version`, then drop it.
pub fn probe(runtime_name: &str) -> Result<Option<String>, JvmError> {
    let jvm = create(runtime_name, &["-Djava.awt.headless=true".to_string()])?;
    jvm.system_property("java.version")
}
