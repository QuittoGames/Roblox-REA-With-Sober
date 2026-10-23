use crate::profile::Profile;
use serde::Serialize;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

pub const APP_ID: &str = "org.vinegarhq.Sober";

#[derive(Debug, Clone, Serialize)]
pub struct RunOutput {
    pub profile: Profile,
    pub command: String,
    pub log_path: PathBuf,
    pub stderr_path: PathBuf,
    pub exit_code: Option<i32>,
    pub success: bool,
    pub extra_environment: Vec<(String, String)>,
}

/// Launch Sober through Flatpak with an explicit, documented profile.
///
/// This is the only function that starts the application. It does not modify
/// the Flatpak installation, the Sober configuration or the host graphics
/// stack; the selected Mesa WSI override is passed to this child process only.
pub fn launch(profile: Profile, app_args: &[String], log_dir: &Path) -> Result<RunOutput, String> {
    fs::create_dir_all(log_dir).map_err(|e| format!("cannot create {}: {e}", log_dir.display()))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let stem = format!("run-{stamp}-{:?}", profile).to_ascii_lowercase();
    let log_path = log_dir.join(format!("{stem}.log"));
    let stderr_path = log_dir.join(format!("{stem}.stderr.log"));
    let stdout = File::create(&log_path)
        .map_err(|e| format!("cannot create {}: {e}", log_path.display()))?;
    let stderr = File::create(&stderr_path)
        .map_err(|e| format!("cannot create {}: {e}", stderr_path.display()))?;

    let envs = profile.env_vars();
    let mut cmd = Command::new("flatpak");
    cmd.arg("run")
        .arg("--command=sober")
        .arg("--die-with-parent");
    for (key, value) in &envs {
        cmd.arg(format!("--env={key}={value}"));
    }
    cmd.arg(APP_ID);
    if !app_args.is_empty() {
        cmd.arg("--");
        cmd.args(app_args);
    }
    cmd.stdout(Stdio::from(stdout)).stderr(Stdio::from(stderr));
    for (key, value) in &envs {
        cmd.env(key, value);
    }

    let rendered = format!(
        "flatpak run --command=sober --die-with-parent {}{}",
        envs.iter()
            .map(|(k, v)| format!("--env={k}={v} "))
            .collect::<String>(),
        APP_ID
    );
    let status = cmd
        .status()
        .map_err(|e| format!("failed to start flatpak: {e}"))?;
    Ok(RunOutput {
        profile,
        command: rendered,
        log_path,
        stderr_path,
        exit_code: status.code(),
        success: status.success(),
        extra_environment: envs
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_environment_is_passed_only_as_child_env() {
        // This test inspects the contract, not the host: baseline must have no
        // override, and a profile must visibly declare the Mesa value.
        assert!(Profile::Baseline.env_vars().is_empty());
        assert_eq!(
            Profile::Fifo.env_vars(),
            vec![("MESA_VK_WSI_PRESENT_MODE", "fifo")]
        );
    }
}
