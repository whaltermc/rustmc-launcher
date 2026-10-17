use rml_version::VersionJson;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Clone, Debug, Serialize)]
pub struct LaunchArgs {
    pub game_dir: PathBuf,
    pub java_bin: PathBuf,
    pub assets_dir: PathBuf,
    pub auth_player_name: String,
    pub auth_uuid: String,
    pub auth_access_token: String,
    pub user_type: String,
    pub version_name: String,
    pub version_type: String,
    pub width: i32,
    pub height: i32,
}

pub fn build_jvm_args(_v: &VersionJson, la: &LaunchArgs) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    args.push("-Xmx1024M".to_string());
    args.push("-Xms512M".to_string());
    args.push(format!("-Djava.library.path={}", rml_version::natives_dir(&la.version_name).display()));
    args.push("-cp".to_string());
    let cp = build_classpath(_v);
    args.push(cp);
    args.push(_v.main_class.clone());
    args.push("--username".to_string());
    args.push(la.auth_player_name.clone());
    args.push("--uuid".to_string());
    args.push(la.auth_uuid.clone());
    args.push("--accessToken".to_string());
    args.push(la.auth_access_token.clone());
    args.push("--version".to_string());
    args.push(la.version_name.clone());
    args.push("--gameDir".to_string());
    args.push(la.game_dir.display().to_string());
    args.push("--assetsDir".to_string());
    args.push(la.assets_dir.display().to_string());
    args.push("--assetIndex".to_string());
    args.push(_v.asset_index.id.clone());
    args.push("--userType".to_string());
    args.push(la.user_type.clone());
    args.push("--width".to_string());
    args.push(la.width.to_string());
    args.push("--height".to_string());
    args.push(la.height.to_string());
    args
}

fn build_classpath(v: &VersionJson) -> String {
    let mut parts: Vec<String> = Vec::new();
    let client = &v.downloads.client;
    let client_jar = rml_version::versions_dir()
        .join(&v.id)
        .join(format!("{}.jar", v.id));
    parts.push(client_jar.display().to_string());
    for lib in &v.libraries {
        if let Some(dl) = &lib.downloads {
            if let Some(art) = &dl.artifact {
                let p = rml_version::libraries_dir().join(&art.path);
                parts.push(p.display().to_string());
            }
        }
    }
    parts.join(if cfg!(windows) { ";" } else { ":" })
}
