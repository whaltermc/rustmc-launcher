use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum VError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("sha1 mismatch")]
    Sha1,
}

const V_MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Deserialize, Debug, Clone)]
pub struct VersionManifest {
    pub versions: Vec<VersionEntry>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct VersionEntry {
    pub id: String,
    pub url: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct VersionJson {
    pub id: String,
    pub inherits_from: Option<String>,
    pub minecraft_arguments: Option<String>,
    pub arguments: Option<Arguments>,
    pub main_class: String,
    pub libraries: Vec<Library>,
    pub assets: String,
    pub asset_index: AssetIndex,
    pub downloads: Downloads,
    pub logging: Option<Logging>,
    pub java_version: Option<JavaVersion>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Arguments {
    pub game: Vec<Arg>,
    pub jvm: Vec<Arg>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum Arg {
    String(String),
    Complex { value: serde_json::Value, rules: Option<Vec<Rule>> },
}

#[derive(Deserialize, Debug, Clone)]
pub struct Rule {
    pub action: String,
    pub os: Option<OsRule>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct OsRule {
    pub name: Option<String>,
    pub arch: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Library {
    pub name: String,
    pub downloads: Option<LibraryDownloads>,
    pub url: Option<String>,
    pub natives: Option<HashMap<String, String>>,
    pub rules: Option<Vec<Rule>>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct LibraryDownloads {
    pub artifact: Option<DownloadArtifact>,
    pub classifiers: Option<HashMap<String, DownloadArtifact>>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct DownloadArtifact {
    pub path: String,
    pub url: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Downloads {
    pub client: DownloadArtifact,
}

#[derive(Deserialize, Debug, Clone)]
pub struct AssetIndex {
    pub id: String,
    pub url: String,
    pub sha1: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Logging {
    pub client: LoggingClient,
}

#[derive(Deserialize, Debug, Clone)]
pub struct LoggingClient {
    pub file: DownloadArtifact,
}

#[derive(Deserialize, Debug, Clone)]
pub struct JavaVersion {
    pub component: String,
    pub major_version: i32,
}

pub fn mc_dir() -> PathBuf {
    rml_core::dirs::data_dir().join(".minecraft")
}

pub fn versions_dir() -> PathBuf {
    mc_dir().join("versions")
}
pub fn libraries_dir() -> PathBuf {
    mc_dir().join("libraries")
}
pub fn assets_dir() -> PathBuf {
    mc_dir().join("assets")
}
pub fn natives_dir(v: &str) -> PathBuf {
    versions_dir().join(v).join(format!("{}-natives", v))
}

pub async fn fetch_manifest(client: &reqwest::Client) -> Result<VersionManifest, VError> {
    let m = client.get(V_MANIFEST).send().await?.json::<VersionManifest>().await?;
    Ok(m)
}

pub async fn fetch_version(client: &reqwest::Client, url: &str) -> Result<VersionJson, VError> {
    let v = client.get(url).send().await?.json::<VersionJson>().await?;
    Ok(v)
}
