use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

pub const APP_ID: &str = "org.vinegarhq.Sober";
pub const REF: &str = "app/org.vinegarhq.Sober/x86_64/stable";

/// Read-only description of the installed Sober application. Paths are kept
/// even when the target is absent so `doctor` can report a missing install
/// instead of silently pretending the environment is complete.
#[derive(Debug, Clone, Serialize)]
pub struct SoberInstall {
    pub app_id: String,
    pub version: Option<String>,
    pub ref_name: String,
    pub commit: Option<String>,
    pub runtime: Option<String>,
    pub app_files: Option<PathBuf>,
    pub user_data: PathBuf,
    pub config_path: PathBuf,
    pub logs_dir: PathBuf,
    pub package_dir: PathBuf,
    pub base_apk: Option<PathBuf>,
    pub split_apk: Option<PathBuf>,
    pub installed: bool,
}

pub fn collect() -> SoberInstall {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let user_data = home.join(".var/app").join(APP_ID);
    let version = command_stdout("flatpak", &["info", "--show-version", APP_ID]);
    let commit = command_stdout("flatpak", &["info", "--show-commit", APP_ID]);
    let location = command_stdout("flatpak", &["info", "--show-location", APP_ID]);
    let info = command_stdout("flatpak", &["info", APP_ID]);
    let runtime = info.as_deref().and_then(parse_runtime);
    let app_files = location.map(|l| PathBuf::from(l).join("files/bin"));
    let package_dir = user_data.join("data/sober/packages/x86_64/com.roblox.client");
    let base_apk = package_dir.join("base.apk");
    let split_apk = package_dir.join("split_config.x86_64.apk");
    SoberInstall {
        app_id: APP_ID.to_string(),
        version,
        ref_name: REF.to_string(),
        commit,
        runtime,
        app_files,
        user_data: user_data.clone(),
        config_path: user_data.join("config/sober/config.json"),
        logs_dir: user_data.join("data/sober/sober_logs"),
        package_dir,
        base_apk: base_apk.exists().then_some(base_apk),
        split_apk: split_apk.exists().then_some(split_apk),
        installed: user_data.exists(),
    }
}

fn command_stdout(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn parse_runtime(info: &str) -> Option<String> {
    for line in info.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("Runtime:") {
            return Some(value.trim().to_string());
        }
    }
    None
}

/// Newest `.log` file in the Sober log directory, by filename (Sober names
/// logs with an ISO-like timestamp prefix).
pub fn latest_log(logs_dir: &std::path::Path) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = fs::read_dir(logs_dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "log"))
        .collect();
    candidates.sort();
    candidates.pop()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_runtime_line() {
        let info = "ID: org.vinegarhq.Sober\nRuntime: org.gnome.Platform/x86_64/51\n";
        assert_eq!(
            parse_runtime(info),
            Some("org.gnome.Platform/x86_64/51".to_string())
        );
    }

    #[test]
    fn ignores_missing_runtime() {
        assert_eq!(parse_runtime("ID: x"), None);
    }
}
