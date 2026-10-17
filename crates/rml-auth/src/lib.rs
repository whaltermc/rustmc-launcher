use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Minecraft (Mojang) public OAuth client id used by the official/third-party launchers.
pub const DEFAULT_CLIENT_ID: &str = "00000000402b5328";

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

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccountKind {
    Microsoft,
    Offline,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    pub kind: AccountKind,
    pub username: String,
    pub uuid: String,
    #[serde(default)]
    pub access_token: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_at: Option<u64>,
}

impl Account {
    /// Deterministic offline (cracked) account. UUID is MD5("OfflinePlayer:<name>") with
    /// version/variant bits set, matching vanilla's offline player UUID scheme.
    pub fn offline(name: &str) -> Self {
        Self {
            kind: AccountKind::Offline,
            username: name.to_string(),
            uuid: offline_uuid(name),
            access_token: None,
            refresh_token: None,
            expires_at: None,
        }
    }

    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(t) => t <= now_unix() + 60,
            None => self.kind == AccountKind::Microsoft,
        }
    }
}

pub fn offline_uuid(name: &str) -> String {
    use md5::{Digest, Md5};
    let mut h = Md5::new();
    h.update(format!("OfflinePlayer:{name}").as_bytes());
    let mut b = h.finalize();
    b[6] = (b[6] & 0x0f) | 0x30;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: Vec<String> = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}{}{}{}-{}{}-{}{}-{}{}-{}{}{}{}{}{}",
        hex[0], hex[1], hex[2], hex[3], hex[4], hex[5], hex[6], hex[7], hex[8], hex[9], hex[10],
        hex[11], hex[12], hex[13], hex[14], hex[15]
    )
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

pub fn accounts_path() -> PathBuf {
    rml_core::dirs::data_dir().join("accounts.json")
}

pub fn load_accounts() -> Vec<Account> {
    std::fs::read_to_string(accounts_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_accounts(accounts: &[Account]) -> Result<(), AuthError> {
    let p = accounts_path();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(p, serde_json::to_string_pretty(accounts)?)?;
    Ok(())
}

pub fn upsert_account(account: &Account) -> Result<Vec<Account>, AuthError> {
    let mut accounts = load_accounts();
    if let Some(slot) = accounts.iter_mut().find(|a| a.uuid == account.uuid) {
        *slot = account.clone();
    } else {
        accounts.push(account.clone());
    }
    save_accounts(&accounts)?;
    Ok(accounts)
}

// ---------------------------------------------------------------------------
// Backwards-compatible AuthData helpers
// ---------------------------------------------------------------------------

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
    std::fs::write(p, serde_json::to_string_pretty(a)?)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Microsoft device-code flow
// ---------------------------------------------------------------------------

const MS_DEVICE_CODE: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const MS_TOKEN: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const XBOX_TOKEN_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XBOX_XSTS_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_AUTH_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";
const SCOPE: &str = "XboxLive.signin offline_access";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceCodeResp {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: i64,
    pub interval: u64,
    pub message: Option<String>,
}

#[derive(Clone, Debug)]
pub enum PollOutcome {
    Pending,
    SlowDown,
    Success(Account),
}

pub struct MicrosoftAuth {
    client: reqwest::Client,
    client_id: String,
}

impl Default for MicrosoftAuth {
    fn default() -> Self {
        Self::new()
    }
}

impl MicrosoftAuth {
    pub fn new() -> Self {
        Self::with_client_id(DEFAULT_CLIENT_ID)
    }

    pub fn with_client_id(client_id: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            client_id: client_id.into(),
        }
    }

    pub async fn request_device_code(&self) -> Result<DeviceCodeResp, AuthError> {
        let form = [("client_id", self.client_id.as_str()), ("scope", SCOPE)];
        Ok(self
            .client
            .post(MS_DEVICE_CODE)
            .form(&form)
            .send()
            .await?
            .json::<DeviceCodeResp>()
            .await?)
    }

    /// One poll iteration; the UI is expected to call this on the advertised interval.
    pub async fn poll(&self, device_code: &str) -> Result<PollOutcome, AuthError> {
        #[derive(Deserialize)]
        struct TokenResp {
            access_token: Option<String>,
            refresh_token: Option<String>,
            expires_in: Option<u64>,
            error: Option<String>,
            error_description: Option<String>,
        }
        let form = [
            ("client_id", self.client_id.as_str()),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", device_code),
        ];
        let tr = self
            .client
            .post(MS_TOKEN)
            .form(&form)
            .send()
            .await?
            .json::<TokenResp>()
            .await?;
        if let Some(err) = tr.error {
            return match err.as_str() {
                "authorization_pending" => Ok(PollOutcome::Pending),
                "slow_down" => Ok(PollOutcome::SlowDown),
                _ => Err(AuthError::Failed(format!(
                    "{err}: {}",
                    tr.error_description.unwrap_or_default()
                ))),
            };
        }
        let ms_access = tr.access_token.ok_or_else(|| AuthError::Failed("no access_token".into()))?;
        let expires_in = tr.expires_in.unwrap_or(3600);
        let refresh = tr.refresh_token;
        let account = self.finish_chain(&ms_access, refresh, expires_in).await?;
        Ok(PollOutcome::Success(account))
    }

    pub async fn refresh(&self, refresh_token: &str) -> Result<Account, AuthError> {
        #[derive(Deserialize)]
        struct TokenResp {
            access_token: String,
            refresh_token: Option<String>,
            expires_in: Option<u64>,
        }
        let form = [
            ("client_id", self.client_id.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("scope", SCOPE),
        ];
        let tr = self
            .client
            .post(MS_TOKEN)
            .form(&form)
            .send()
            .await?
            .json::<TokenResp>()
            .await?;
        self.finish_chain(
            &tr.access_token,
            tr.refresh_token.or_else(|| Some(refresh_token.to_string())),
            tr.expires_in.unwrap_or(3600),
        )
        .await
    }

    async fn finish_chain(
        &self,
        ms_access: &str,
        refresh_token: Option<String>,
        expires_in: u64,
    ) -> Result<Account, AuthError> {
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
        #[derive(Deserialize)]
        struct XboxAuthResp {
            #[serde(rename = "Token")]
            token: String,
        }
        let xbox = self
            .client
            .post(XBOX_TOKEN_URL)
            .header("Accept", "application/json")
            .json(&XboxAuthReq {
                properties: XboxProperties {
                    auth_method: "RPS",
                    site_name: "user.auth.xboxlive.com",
                    rps_ticket: format!("d={ms_access}"),
                },
                relying_party: "http://auth.xboxlive.com",
                token_type: "JWT",
            })
            .send()
            .await?
            .json::<XboxAuthResp>()
            .await?;

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
        #[derive(Deserialize)]
        struct XboxXstsResp {
            #[serde(rename = "Token")]
            token: String,
            #[serde(rename = "DisplayClaims")]
            display_claims: XboxDisplayClaims,
        }
        #[derive(Deserialize)]
        struct XboxDisplayClaims {
            xui: Vec<XuiClaim>,
        }
        #[derive(Deserialize)]
        struct XuiClaim {
            uhs: String,
        }
        let xsts = self
            .client
            .post(XBOX_XSTS_URL)
            .header("Accept", "application/json")
            .json(&XboxXstsReq {
                properties: XboxXstsProperties {
                    sandbox_id: "RETAIL",
                    user_tokens: vec![xbox.token],
                },
                relying_party: "rp://api.minecraftservices.com/",
                token_type: "JWT",
            })
            .send()
            .await?
            .json::<XboxXstsResp>()
            .await?;
        let uhs = xsts
            .display_claims
            .xui
            .first()
            .map(|c| c.uhs.clone())
            .ok_or_else(|| AuthError::Failed("no xui claim".into()))?;
        let identity = format!("XBL3.0 x={uhs};{}", xsts.token);

        #[derive(Serialize)]
        struct McLoginReq {
            identity_token: String,
        }
        #[derive(Deserialize)]
        struct McLoginResp {
            access_token: String,
        }
        let mc = self
            .client
            .post(MC_AUTH_URL)
            .json(&McLoginReq { identity_token: identity })
            .send()
            .await?
            .json::<McLoginResp>()
            .await?;

        #[derive(Deserialize)]
        struct Profile {
            id: String,
            name: String,
        }
        let profile = self
            .client
            .get(MC_PROFILE_URL)
            .bearer_auth(&mc.access_token)
            .send()
            .await?
            .json::<Profile>()
            .await?;

        Ok(Account {
            kind: AccountKind::Microsoft,
            username: profile.name,
            uuid: profile.id,
            access_token: Some(mc.access_token),
            refresh_token,
            expires_at: Some(now_unix() + expires_in),
        })
    }
}

/// Convenience loop used by the desktop/ego UI. Returns the device code plus a future
/// that resolves once the user approves on the Microsoft page.
pub async fn device_code_flow(
) -> Result<(DeviceCodeResp, impl std::future::Future<Output = Result<AuthData, AuthError>> + Send), AuthError>
{
    let auth = MicrosoftAuth::new();
    let device = auth.request_device_code().await?;
    let code = device.device_code.clone();
    let interval = device.interval.max(5);
    let expires_in = device.expires_in.max(60) as u64;
    let fut = async move {
        let start = std::time::Instant::now();
        loop {
            if start.elapsed().as_secs() > expires_in {
                return Err(AuthError::PollTimeout);
            }
            tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
            match auth.poll(&code).await? {
                PollOutcome::Success(a) => {
                    let data = AuthData {
                        access_token: a.access_token.clone().unwrap_or_default(),
                        username: a.username.clone(),
                        uuid: a.uuid.clone(),
                    };
                    let _ = upsert_account(&a);
                    return Ok(data);
                }
                PollOutcome::Pending | PollOutcome::SlowDown => {}
            }
        }
    };
    Ok((device, fut))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_uuid_is_stable_and_valid() {
        let a = Account::offline("Steve");
        let b = Account::offline("Steve");
        assert_eq!(a.uuid, b.uuid);
        assert_eq!(a.uuid.len(), 36);
        assert_eq!(&a.uuid[14..15], "3", "version nibble");
        let v: u8 = u8::from_str_radix(&a.uuid[19..20], 16).unwrap();
        assert!(v >= 8, "variant bits set");
        assert_ne!(Account::offline("Alex").uuid, a.uuid);
    }

    #[test]
    fn offline_account_roundtrip() {
        let dir = std::env::temp_dir().join(format!("rml-test-{}", std::process::id()));
        std::env::set_var("HOME", &dir);
        let acc = Account::offline("Player1");
        let list = upsert_account(&acc).unwrap();
        assert!(list.iter().any(|a| a.username == "Player1"));
        // upsert same uuid should not duplicate
        let list = upsert_account(&acc).unwrap();
        assert_eq!(list.iter().filter(|a| a.uuid == acc.uuid).count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
