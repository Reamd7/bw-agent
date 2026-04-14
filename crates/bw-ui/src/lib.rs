use eframe::egui;
use std::sync::{Arc, Mutex};

/// Show a modal password prompt window. Blocks until the user submits or cancels.
/// Returns `Some(password)` on submit, `None` on cancel/close.
pub fn prompt_master_password() -> Option<String> {
    let result: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let result_clone = result.clone();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 180.0])
            .with_resizable(false)
            .with_always_on_top(),
        ..Default::default()
    };

    let _ = eframe::run_native(
        "Bitwarden SSH Agent - Unlock",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(PasswordApp {
                password: String::new(),
                result: result_clone,
                error_msg: None,
            }))
        }),
    );

    let lock = result.lock().unwrap();
    lock.clone()
}

struct PasswordApp {
    password: String,
    result: Arc<Mutex<Option<String>>>,
    error_msg: Option<String>,
}

impl eframe::App for PasswordApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ui.vertical_centered(|ui| {
            ui.add_space(10.0);
            ui.heading("Bitwarden Master Password");
            ui.add_space(10.0);

            ui.label("Enter your master password to unlock SSH keys:");
            ui.add_space(5.0);

            let response = ui.add(
                egui::TextEdit::singleline(&mut self.password)
                    .password(true)
                    .desired_width(300.0)
                    .hint_text("Master Password"),
            );

            // Auto-focus the password field
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.submit(&ctx);
                return;
            }
            response.request_focus();

            if let Some(msg) = &self.error_msg {
                ui.colored_label(egui::Color32::RED, msg);
            }

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Unlock").clicked() {
                    self.submit(&ctx);
                }
                if ui.button("Cancel").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });
    }
}

impl PasswordApp {
    fn submit(&mut self, ctx: &egui::Context) {
        if self.password.is_empty() {
            self.error_msg = Some("Password cannot be empty".to_string());
            return;
        }
        *self.result.lock().unwrap() = Some(self.password.clone());
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}
