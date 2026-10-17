use eframe::egui;
use rml_auth::AuthData;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, Default)]
pub struct UiState {
    pub logged_in: bool,
    pub username: String,
    pub device_code_msg: String,
}

pub struct LauncherApp {
    state: UiState,
    device_flow: Option<(rml_auth::DeviceCodeResp, tokio::task::JoinHandle<Result<AuthData, rml_auth::AuthError>>)>,
    rt: tokio::runtime::Handle,
}

impl LauncherApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        if let Some(s) = cc.storage {
            let st = eframe::get_value(s, eframe::APP_KEY).unwrap_or_default();
            return Self {
                state: st,
                device_flow: None,
                rt: tokio::runtime::Handle::current(),
            };
        }
        Self {
            state: UiState::default(),
            device_flow: None,
            rt: tokio::runtime::Handle::current(),
        }
    }
}

impl eframe::App for LauncherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("RustMC Launcher");
            if self.state.logged_in {
                ui.label(format!("Logged in as: {}", self.state.username));
            } else {
                if let Some((d, _h)) = &self.device_flow {
                    ui.label(&d.message.clone().unwrap_or_default());
                    ui.label(format!("Code: {}", d.user_code));
                    ui.hyperlink(&d.verification_uri);
                }
                if self.device_flow.is_none() {
                    if ui.button("Login with Microsoft (Device Flow)").clicked() {
                        let handle = self.rt.clone();
                        let (d, fut) = self.rt.block_on(rml_auth::device_code_flow()).unwrap();
                        self.state.device_code_msg = d.message.clone().unwrap_or_default();
                        let h = handle.spawn(async move { fut.await });
                        self.device_flow = Some((d, h));
                    }
                } else {
                    ui.spinner();
                }
            }
            if let Some((_d, h)) = self.device_flow.take() {
                if h.is_finished() {
                    let res = self.rt.block_on(h).unwrap();
                    match res {
                        Ok(a) => {
                            self.state.logged_in = true;
                            self.state.username = a.username.clone();
                            let _ = rml_auth::save_auth(&a);
                        }
                        Err(e) => {
                            self.state.device_code_msg = format!("error: {}", e);
                        }
                    }
                } else {
                    self.device_flow = Some((_d, h));
                }
            }
        });
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.state);
    }
}
