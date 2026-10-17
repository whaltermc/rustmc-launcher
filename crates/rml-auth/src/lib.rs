use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("poll timeout")]
    PollTimeout,
    #[error("auth failed: {0}")]
    Failed(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct XboxToken {
    pub token: String,
    pub expires_in: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MinecraftProfile {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuthData {
    pub access_token: String,
    pub username: String,
    pub uuid: String,
}

pub fn auth_cache_path() -> PathBuf {
    rml_core::dirs::data_dir().join("auth.json")
}

pub fn load_auth() -> Option<AuthData> {
    let p = auth_cache_path();
    std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok())
}

pub fn save_auth(a: &AuthData) -> Result<(), AuthError> {
    let p = auth_cache_path();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    let mut f = std::fs::File::create(p)?;
    f.write_all(serde_json::to_string_pretty(a)?.as_bytes())?;
    Ok(())
}

#[derive(Serialize)]
struct DeviceCodeReq {
    client_id: &'static str,
    scope: &'static str,
}

#[derive(Clone, Deserialize, Debug)]
pub struct DeviceCodeResp {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: i64,
    pub interval: u64,
    pub message: Option<String>,
}

#[derive(Serialize)]
struct TokenPollReq {
    client_id: &'static str,
    grant_type: &'static str,
    device_code: String,
}

#[derive(Clone, Deserialize, Debug)]
struct TokenResp {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    error: Option<String>,
    error_description: Option<String>,
}

const MS_DEVICE_CODE: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const MS_TOKEN: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const XBOX_TOKEN_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XBOX_XSTS_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_AUTH_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";

#[derive(Serialize)]
struct XboxAuthReq {
    #[serde(rename = "Properties")]
    properties: XboxProperties,
    #[serde(rename = "RelyingParty")]
    relying_party: &'static str,
    #[serde(rename = "TokenType")]
    token_type: &'static str,
}

#[derive(Serialize)]
struct XboxProperties {
    #[serde(rename = "AuthMethod")]
    auth_method: &'static str,
    #[serde(rename = "SiteName")]
    site_name: &'static str,
    #[serde(rename = "RpsTicket")]
    rps_ticket: String,
}

#[derive(Clone, Deserialize, Debug)]
struct XboxAuthResp {
    #[serde(rename = "Token")]
    token: String,
}

#[derive(Serialize)]
struct XboxXstsReq {
    #[serde(rename = "Properties")]
    properties: XboxXstsProperties,
    #[serde(rename = "RelyingParty")]
    relying_party: &'static str,
    #[serde(rename = "TokenType")]
    token_type: &'static str,
}

#[derive(Serialize)]
struct XboxXstsProperties {
    #[serde(rename = "SandboxId")]
    sandbox_id: &'static str,
    #[serde(rename = "UserTokens")]
    user_tokens: Vec<String>,
}

#[derive(Clone, Deserialize, Debug)]
struct XboxXstsResp {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: XboxDisplayClaims,
}

#[derive(Clone, Deserialize, Debug)]
struct XboxDisplayClaims {
    #[serde(rename = "xui")]
    xui: Vec<XuiClaim>,
}

#[derive(Clone, Deserialize, Debug)]
struct XuiClaim {
    uhs: String,
}

#[derive(Serialize)]
struct McLoginReq {
    identity_token: String,
}

#[derive(Clone, Deserialize, Debug)]
struct McLoginResp {
    access_token: String,
}

pub async fn device_code_flow() -> Result<(DeviceCodeResp, impl std::future::Future<Output = Result<AuthData, AuthError>> + Send), AuthError> {
    let client = reqwest::Client::new();
    let d = client
        .post(MS_DEVICE_CODE)
        .form(&DeviceCodeReq {
            client_id: "00000000-0000-0000-0000-000000000000",
            scope: "XboxLive.signin offline_access",
        })
        .send()
        .await?
        .json::<DeviceCodeResp>()
        .await?;
    let device = d.clone();
    let fut = async move {
        let mut poll = tokio::time::interval(std::time::Duration::from_secs(device.interval.max(5)));
        let expires = tokio::time::Instant::now() + std::time::Duration::from_secs(device.expires_in.max(60) as u64);
        loop {
            poll.tick().await;
            if tokio::time::Instant::now() > expires {
                return Err(AuthError::PollTimeout);
            }
            let tr = client
                .post(MS_TOKEN)
                .form(&TokenPollReq {
                    client_id: "00000000-0000-0000-0000-000000000000",
                    grant_type: "urn:ietf:params:oauth:grant-type:device_code",
                    device_code: device.device_code.clone(),
                })
                .send()
                .await?
                .json::<TokenResp>()
                .await?;
            if let Some(err) = tr.error {
                if err == "authorization_pending" || err == "slow_down" {
                    continue;
                }
                return Err(AuthError::Failed(format!("{}: {:?}", err, tr.error_description)));
            }
            let ms_access = tr.access_token.ok_or_else(|| AuthError::Failed("no access_token".into()))?;
            let xbox = client
                .post(XBOX_TOKEN_URL)
                .header("Content-Type", "application/json")
                .header("Accept", "application/json")
                .json(&XboxAuthReq {
                    properties: XboxProperties {
                        auth_method: "RPS",
                        site_name: "user.auth.xboxlive.com",
                        rps_ticket: format!("d={}", ms_access),
                    },
                    relying_party: "http://auth.xboxlive.com",
                    token_type: "JWT",
                })
                .send()
                .await?
                .json::<XboxAuthResp>()
                .await?;
            let xsts = client
                .post(XBOX_XSTS_URL)
                .header("Content-Type", "application/json")
                .json(&XboxXstsReq {
                    properties: XboxXstsProperties {
                        sandbox_id: "RETAIL",
                        user_tokens: vec![xbox.token.clone()],
                    },
                    relying_party: "rp://api.minecraftservices.com/",
                    token_type: "JWT",
                })
                .send()
                .await?
                .json::<XboxXstsResp>()
                .await?;
            let uhs = xsts.display_claims.xui.get(0).map(|c| c.uhs.clone()).ok_or_else(|| AuthError::Failed("no xui".into()))?;
            let identity = format!("XBL3.0 x={};{}", uhs, xsts.token);
            let mcl = client
                .post(MC_AUTH_URL)
                .header("Content-Type", "application/json")
                .json(&McLoginReq { identity_token: identity })
                .send()
                .await?
                .json::<McLoginResp>()
                .await?;
            let prof = client
                .get(MC_PROFILE_URL)
                .header("Authorization", format!("Bearer {}", mcl.access_token))
                .send()
                .await?
                .json::<MinecraftProfile>()
                .await?;
            return Ok(AuthData {
                access_token: mcl.access_token,
                username: prof.name,
                uuid: prof.id,
            });
        }
    };
    Ok((d, fut))
}
