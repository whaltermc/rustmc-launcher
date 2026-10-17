//! Java runtime acquisition for Android.
//!
//! Runtimes are the prebuilt Pojav/MojoLauncher-style OpenJDK bundles hosted at
//! `https://mojolauncher.github.io/jre-download/`. Each bundle is served as two
//! xz-compressed tarballs that are overlaid on top of each other:
//!
//! * `universal.tar.xz`     — architecture independent files (`conf/`, `lib/modules`, ...)
//! * `bin-<arch>.tar.xz`    — architecture specific files (`bin/`, `lib/server/libjvm.so`, ...)
//!
//! The `version` document is a newline separated list of `name:base64signature` pairs and is
//! also used as the update marker.

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("xz: {0}")]
    Xz(#[from] lzma_rs::error::Error),
    #[error("unknown runtime '{0}'")]
    UnknownRuntime(String),
    #[error("version bundle missing component '{0}'")]
    MissingComponent(String),
}

pub const RUNTIME_BASE: &str = "https://mojolauncher.github.io/jre-download/";

#[derive(Clone, Copy, Debug)]
pub struct RuntimeSpec {
    pub name: &'static str,
    pub major: u32,
    pub path: &'static str,
}

pub const RUNTIMES: &[RuntimeSpec] = &[
    RuntimeSpec { name: "Internal-17", major: 17, path: "components/jre-new" },
    RuntimeSpec { name: "Internal-21", major: 21, path: "components/jre-21" },
    RuntimeSpec { name: "Internal-25", major: 25, path: "components/jre-25" },
];

pub fn spec(name: &str) -> Option<&'static RuntimeSpec> {
    RUNTIMES.iter().find(|r| r.name == name)
}

pub fn runtime_dir(name: &str) -> std::path::PathBuf {
    rml_core::dirs::data_dir().join("runtimes").join(name)
}

pub fn libjvm_path(name: &str) -> std::path::PathBuf {
    runtime_dir(name).join("lib/server/libjvm.so")
}

pub fn cache_dir() -> std::path::PathBuf {
    rml_core::dirs::cache_dir()
}

/// Map an ABI arch (`aarch64`, `arm`, `x86_64`, `x86`) to the bundle suffix used by the host
/// (`arm64`, `arm32`, `x64`, `x86`).
pub fn arch_suffix() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "arm" => "arm32",
        "x86_64" => "x64",
        "x86" => "x86",
        other => other,
    }
}

/// Parse a `version` document into `(component_name, signature)` pairs.
pub fn parse_version_bundle(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let (name, sig) = line.split_once(':')?;
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            Some((name.to_string(), sig.trim().to_string()))
        })
        .collect()
}

fn client() -> Result<reqwest::blocking::Client, RuntimeError> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent("rustmc-launcher")
        .build()?)
}

/// Fetch the remote `version` document for a runtime.
pub fn fetch_remote_version(spec: &RuntimeSpec) -> Result<String, RuntimeError> {
    let url = format!("{}{}/version", RUNTIME_BASE, spec.path);
    Ok(client()?.get(url).send()?.error_for_status()?.text()?)
}

/// Download `url` to `dest`, reporting `(downloaded, total, label)` progress.
pub fn download(
    url: &str,
    dest: &std::path::Path,
    label: &str,
    progress: &dyn Fn(u64, u64, &str),
) -> Result<(), RuntimeError> {
    use std::io::{Read, Write};
    let mut resp = client()?.get(url).send()?.error_for_status()?;
    let total = resp.content_length().unwrap_or(0);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::File::create(dest)?;
    let mut buf = [0u8; 64 * 1024];
    let mut done = 0u64;
    progress(0, total, label);
    loop {
        let n = resp.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        done += n as u64;
        progress(done, total, label);
    }
    file.flush()?;
    Ok(())
}

/// Install a runtime, returning its directory.
pub fn install(
    spec: &RuntimeSpec,
    progress: &dyn Fn(u64, u64, &str),
) -> Result<std::path::PathBuf, RuntimeError> {
    let version_text = fetch_remote_version(spec)?;
    install_from_version(spec, &version_text, progress)
}

/// Install using an already-fetched `version` bundle.
pub fn install_from_version(
    spec: &RuntimeSpec,
    version_text: &str,
    progress: &dyn Fn(u64, u64, &str),
) -> Result<std::path::PathBuf, RuntimeError> {
    let components = parse_version_bundle(version_text);
    let has = |name: &str| components.iter().any(|(n, _)| n == name);
    if !has("universal.tar.xz") {
        return Err(RuntimeError::MissingComponent("universal.tar.xz".into()));
    }
    let platform = format!("bin-{}.tar.xz", arch_suffix());
    if !has(&platform) {
        return Err(RuntimeError::MissingComponent(platform));
    }

    let dir = runtime_dir(spec.name);
    let cache = cache_dir().join(spec.name);
    std::fs::create_dir_all(&cache)?;
    let universal = cache.join("universal.tar.xz");
    let platform_file = cache.join(&platform);
    let base = format!("{}{}/", RUNTIME_BASE, spec.path);

    download(&format!("{base}universal.tar.xz"), &universal, "java-runtime", progress)?;
    download(&format!("{base}{platform}"), &platform_file, "java-platform", progress)?;

    unpack_tar_xz_over(&universal, &dir, "universal")?;
    unpack_tar_xz_over(&platform_file, &dir, "platform")?;
    fix_permissions(&dir)?;
    std::fs::write(dir.join(".rml-version"), version_text)?;
    let _ = std::fs::remove_file(&universal);
    let _ = std::fs::remove_file(&platform_file);
    Ok(dir)
}

/// Decompress `archive_path` (xz) and unpack every entry into `dest`, replacing files.
///
/// Uses a pure-Rust xz decoder so the resulting `.so` has no dependency on a system
/// `liblzma`, which Android's app sandbox does not provide.
fn unpack_tar_xz_over(
    archive_path: &std::path::Path,
    dest: &std::path::Path,
    tag: &str,
) -> Result<(), RuntimeError> {
    use std::io::Write;
    std::fs::create_dir_all(dest)?;
    let cache = cache_dir();
    std::fs::create_dir_all(&cache)?;
    let tar_path = cache.join(format!("{}-{}.tar", std::process::id(), tag));
    {
        let mut input = std::io::BufReader::new(std::fs::File::open(archive_path)?);
        let mut output = std::io::BufWriter::new(std::fs::File::create(&tar_path)?);
        lzma_rs::xz_decompress(&mut input, &mut output)?;
        output.flush()?;
    }
    let file = std::fs::File::open(&tar_path)?;
    let mut archive = tar::Archive::new(file);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let rel = path.strip_prefix("./").unwrap_or(&path);
        if rel.as_os_str().is_empty() {
            continue;
        }
        if rel
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            continue;
        }
        let out = dest.join(rel);
        if entry.header().entry_type().is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        entry.unpack(&out)?;
    }
    let _ = std::fs::remove_file(&tar_path);
    Ok(())
}

/// Make executables/shared objects usable on Android's filesystem.
fn fix_permissions(dir: &std::path::Path) -> Result<(), RuntimeError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut stack = vec![dir.to_path_buf()];
        while let Some(p) = stack.pop() {
            for entry in std::fs::read_dir(&p)? {
                let entry = entry?;
                let path = entry.path();
                if entry.file_type()?.is_dir() {
                    stack.push(path);
                    continue;
                }
                let name = entry.file_name();
                let name = name.to_string_lossy();
                let in_bin = path.parent().map(|d| d.ends_with("bin")).unwrap_or(false);
                if in_bin || name.ends_with(".so") || name == "jexec" {
                    let mut perms = entry.metadata()?.permissions();
                    perms.set_mode(0o755);
                    std::fs::set_permissions(&path, perms)?;
                }
            }
        }
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct InstalledRuntime {
    pub name: String,
    pub java_version: Option<String>,
    pub major: Option<u32>,
    pub dir: std::path::PathBuf,
    pub libjvm: std::path::PathBuf,
}

impl InstalledRuntime {
    pub fn is_complete(&self) -> bool {
        self.libjvm.exists()
    }
}

fn parse_release_major(release: &str) -> (Option<String>, Option<u32>) {
    let mut version = None;
    for line in release.lines() {
        if let Some(v) = line.strip_prefix("JAVA_VERSION=") {
            version = Some(v.trim_matches('"').to_string());
        }
    }
    let major = version.as_ref().and_then(|v| {
        if let Some(rest) = v.strip_prefix("1.") {
            rest.split('.').next().and_then(|s| s.parse().ok())
        } else {
            v.split(['.', '-']).next().and_then(|s| s.parse().ok())
        }
    });
    (version, major)
}

/// Enumerate runtimes from both the static table and disk.
pub fn installed() -> Vec<InstalledRuntime> {
    let mut names: Vec<String> = RUNTIMES.iter().map(|r| r.name.to_string()).collect();
    if let Ok(entries) = std::fs::read_dir(rml_core::dirs::data_dir().join("runtimes")) {
        for e in entries.flatten() {
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let n = e.file_name().to_string_lossy().to_string();
                if !names.contains(&n) {
                    names.push(n);
                }
            }
        }
    }
    names
        .into_iter()
        .map(|name| {
            let dir = runtime_dir(&name);
            let release = std::fs::read_to_string(dir.join("release")).unwrap_or_default();
            let (java_version, major) = parse_release_major(&release);
            let major = major.or_else(|| spec(&name).map(|s| s.major));
            let libjvm = dir.join("lib/server/libjvm.so");
            InstalledRuntime {
                name,
                java_version,
                major,
                dir,
                libjvm,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_parses_components() {
        let text = "universal.tar.xz:AAAA\nbin-arm64.tar.xz:BBBB\n\n";
        let comps = parse_version_bundle(text);
        assert_eq!(comps.len(), 2);
        assert_eq!(comps[0].0, "universal.tar.xz");
        assert_eq!(comps[1].0, "bin-arm64.tar.xz");
    }

    #[test]
    fn release_major_parsing() {
        let (v, m) = parse_release_major("JAVA_VERSION=\"21.0.12\"\n");
        assert_eq!(v.as_deref(), Some("21.0.12"));
        assert_eq!(m, Some(21));
        let (_, m) = parse_release_major("JAVA_VERSION=\"1.8.0_402\"\n");
        assert_eq!(m, Some(8));
    }
}
