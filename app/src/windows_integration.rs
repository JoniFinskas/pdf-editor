//! Explicit per-user Windows shell registration. Never writes UserChoice or HKLM.
use eframe::egui;
use std::{io, path::Path, process::Command};
const APP: &str = r"HKCU\Software\Classes\PdfEditor.Pdf\Capabilities";
const PROG: &str = r"HKCU\Software\Classes\PdfEditor.Pdf";
const MENU: &str = r"HKCU\Software\Classes\SystemFileAssociations\.pdf\shell\PdfEditor";
const REGISTERED: &str = r"HKCU\Software\RegisteredApplications";
#[derive(Debug)]
struct Value {
    key: String,
    name: String,
    data: String,
}
fn value(key: impl Into<String>, name: &str, data: impl Into<String>) -> Value {
    Value {
        key: key.into(),
        name: name.into(),
        data: data.into(),
    }
}
fn quoted(path: &Path) -> io::Result<String> {
    let text = path
        .to_str()
        .ok_or_else(|| io::Error::other("Executable path is not Unicode"))?;
    if !path.is_absolute() || text.contains(['"', '\r', '\n', '\0']) {
        return Err(io::Error::other("Invalid executable path"));
    }
    Ok(format!("\"{text}\""))
}
fn registration(exe: &Path) -> io::Result<Vec<Value>> {
    Ok(vec![
        value(APP, "ApplicationName", "PDF Editor"),
        value(APP, "ApplicationDescription", "Local PDF editor"),
        value(format!(r"{APP}\FileAssociations"), ".pdf", "PdfEditor.Pdf"),
        value(
            REGISTERED,
            "PdfEditor",
            r"Software\Classes\PdfEditor.Pdf\Capabilities",
        ),
        value(PROG, "", "PDF document"),
        value(
            format!(r"{PROG}\shell\open\command"),
            "",
            format!("{} \"%1\"", quoted(exe)?),
        ),
    ])
}
fn menu(exe: &Path) -> io::Result<Vec<Value>> {
    let cli = exe.with_file_name("pdf-editor-cli.exe");
    let mut values = vec![
        value(MENU, "MUIVerb", "PDF Editor"),
        value(MENU, "SubCommands", ""),
    ];
    for (key, label, action) in [
        ("01open", "Open in tab", "shell-open"),
        ("02merge", "Merge selected", "shell-merge"),
        ("03compress", "Compress", "shell-compress"),
        ("04export", "Export as images", "shell-export"),
    ] {
        let root = format!(r"{MENU}\shell\{key}");
        values.push(value(&root, "MUIVerb", label));
        values.push(value(&root, "MultiSelectModel", "Document"));
        values.push(value(
            format!(r"{root}\command"),
            "",
            format!("{} {action} \"%1\"", quoted(&cli)?),
        ));
    }
    Ok(values)
}
fn reg() -> Command {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let mut command = Command::new(Path::new(&root).join("System32/reg.exe"));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}
fn install(values: Vec<Value>) -> io::Result<()> {
    for v in values {
        let mut command = reg();
        command.args(["add", &v.key, "/t", "REG_SZ", "/d", &v.data, "/f"]);
        if v.name.is_empty() {
            command.arg("/ve");
        } else {
            command.args(["/v", &v.name]);
        }
        if !command.output()?.status.success() {
            return Err(io::Error::other("Windows registration failed"));
        }
    }
    Ok(())
}
fn remove(key: &str, name: Option<&str>) -> io::Result<()> {
    let mut query = reg();
    query.args(["query", key]);
    if let Some(name) = name {
        query.args(["/v", name]);
    }
    if !query.output()?.status.success() {
        return Ok(());
    }
    let mut command = reg();
    command.args(["delete", key, "/f"]);
    if let Some(name) = name {
        command.args(["/v", name]);
    }
    if !command.output()?.status.success() {
        return Err(io::Error::other("Windows registration removal failed"));
    }
    Ok(())
}
#[derive(Default)]
pub struct Integration {
    status: String,
    menu_enabled: Option<bool>,
}
impl Integration {
    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.heading("Windows integration");
        ui.label("Register for this user, then choose PDF Editor for .pdf in Windows Settings.");
        ui.horizontal(|ui| {
            if ui.button("Make default PDF app").clicked() {
                self.result((|| {
                    install(registration(&std::env::current_exe()?)?)?;
                    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
                    Command::new(Path::new(&root).join("explorer.exe")).arg("ms-settings:defaultapps").spawn()?;
                    Ok(())
                })(), "Registered. Choose PDF Editor in Windows Default Apps.");
            }
            if ui.button("Remove").clicked() {
                self.result((|| { remove(MENU, None)?; remove(PROG, None)?; remove(APP, None)?; remove(REGISTERED, Some("PdfEditor")) })(), "Registration removed. Choose another default in Windows Settings if necessary.");
                self.menu_enabled = None;
            }
        });
        // Query only when this settings pane is first shown or after removal.
        let mut enabled = *self.menu_enabled.get_or_insert_with(|| {
            reg()
                .args(["query", MENU])
                .output()
                .is_ok_and(|o| o.status.success())
        });
        if ui
            .checkbox(
                &mut enabled,
                "Explorer PDF menu: Open, Merge, Compress, Export",
            )
            .changed()
        {
            let result = if enabled {
                (|| {
                    let exe = std::env::current_exe()?;
                    if !exe.with_file_name("pdf-editor-cli.exe").is_file() {
                        return Err(io::Error::other(
                            "Build or install pdf-editor-cli.exe beside the app first",
                        ));
                    }
                    install(menu(&exe)?)
                })()
            } else {
                remove(MENU, None)
            };
            if result.is_ok() {
                self.menu_enabled = Some(enabled);
            }
            self.result(
                result,
                "Explorer menu updated. On Windows 11 use Show more options.",
            );
        }
        if !self.status.is_empty() {
            ui.label(&self.status);
        }
    }
    fn result(&mut self, result: io::Result<()>, success: &str) {
        self.status = result.map_or_else(|e| e.to_string(), |_| success.into());
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn records_are_per_user_and_quote_paths() {
        let exe = std::env::temp_dir().join("Space & name/pdf-editor.exe");
        let records = registration(&exe)
            .unwrap()
            .into_iter()
            .chain(menu(&exe).unwrap());
        for v in records {
            assert!(v.key.starts_with("HKCU\\"));
            assert!(!v.key.contains("UserChoice"));
            if v.key.ends_with("command") {
                assert!(v.data.starts_with('"'));
                assert!(v.data.ends_with("\"%1\""));
                assert!(!v.data.contains("cmd.exe"));
            }
        }
        assert!(quoted(Path::new("relative.exe")).is_err());
    }
    #[test]
    fn all_explorer_actions_use_console_wrapper() {
        let records = menu(&std::env::temp_dir().join("pdf-editor.exe")).unwrap();
        let commands: Vec<_> = records
            .iter()
            .filter(|v| v.key.ends_with("command"))
            .collect();
        assert_eq!(commands.len(), 4);
        for action in [
            "shell-open",
            "shell-merge",
            "shell-compress",
            "shell-export",
        ] {
            assert!(
                commands
                    .iter()
                    .any(|v| v.data.contains(action) && v.data.contains("pdf-editor-cli.exe"))
            );
        }
    }
}
