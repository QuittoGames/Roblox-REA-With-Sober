use serde::Serialize;
use std::str::FromStr;

/// Launch profiles used for controlled A/B experiments.
///
/// The current Sober log shows `VK_PRESENT_MODE_FIFO_KHR` (mode 2) with four
/// swapchain images. These profiles change only the documented Mesa WSI
/// present-mode override; they do not patch Sober, Roblox, the driver or the
/// security boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    Baseline,
    Fifo,
    Relaxed,
    Mailbox,
    Immediate,
    Llvmpipe,
}

impl Profile {
    pub const ALL: [Profile; 6] = [
        Profile::Baseline,
        Profile::Fifo,
        Profile::Relaxed,
        Profile::Mailbox,
        Profile::Immediate,
        Profile::Llvmpipe,
    ];

    pub fn description(self) -> &'static str {
        match self {
            Profile::Baseline => {
                "Sem override de present mode; usa o comportamento padrao do driver."
            }
            Profile::Fifo => "Forca VK_PRESENT_MODE_FIFO_KHR via MESA_VK_WSI_PRESENT_MODE=fifo.",
            Profile::Relaxed => {
                "Permite tearing controlado do WSI: MESA_VK_WSI_PRESENT_MODE=relaxed."
            }
            Profile::Mailbox => "Solicita present mode mailbox (o driver pode nao suportar).",
            Profile::Immediate => {
                "Solicita present mode immediate; diagnostico de latencia, sem vsync."
            }
            Profile::Llvmpipe => {
                "Diagnostico de CPU/llvmpipe; nao e um caminho de desempenho para o jogo."
            }
        }
    }

    /// Environment variables passed to the Sober process only. The caller's
    /// shell environment is never modified.
    pub fn env_vars(self) -> Vec<(&'static str, &'static str)> {
        match self {
            Profile::Baseline => Vec::new(),
            Profile::Fifo => vec![("MESA_VK_WSI_PRESENT_MODE", "fifo")],
            Profile::Relaxed => vec![("MESA_VK_WSI_PRESENT_MODE", "relaxed")],
            Profile::Mailbox => vec![("MESA_VK_WSI_PRESENT_MODE", "mailbox")],
            Profile::Immediate => vec![("MESA_VK_WSI_PRESENT_MODE", "immediate")],
            Profile::Llvmpipe => vec![
                ("LIBGL_ALWAYS_SOFTWARE", "true"),
                ("MESA_LOADER_DRIVER_OVERRIDE", "llvmpipe"),
            ],
        }
    }

    pub fn is_diagnostic_only(self) -> bool {
        matches!(self, Profile::Llvmpipe)
    }
}

impl FromStr for Profile {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "baseline" | "default" => Ok(Profile::Baseline),
            "fifo" | "vsync" => Ok(Profile::Fifo),
            "relaxed" => Ok(Profile::Relaxed),
            "mailbox" => Ok(Profile::Mailbox),
            "immediate" | "no-vsync" => Ok(Profile::Immediate),
            "llvmpipe" | "software" => Ok(Profile::Llvmpipe),
            other => Err(format!(
                "perfil desconhecido: {other} (use baseline|fifo|relaxed|mailbox|immediate|llvmpipe)"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_has_no_overrides() {
        assert!(Profile::Baseline.env_vars().is_empty());
    }

    #[test]
    fn profiles_map_to_documented_mesa_values() {
        assert_eq!(
            Profile::Fifo.env_vars(),
            vec![("MESA_VK_WSI_PRESENT_MODE", "fifo")]
        );
        assert_eq!(
            Profile::Immediate.env_vars(),
            vec![("MESA_VK_WSI_PRESENT_MODE", "immediate")]
        );
    }

    #[test]
    fn parses_aliases() {
        assert_eq!("default".parse::<Profile>().unwrap(), Profile::Baseline);
        assert_eq!("vsync".parse::<Profile>().unwrap(), Profile::Fifo);
        assert!("nope".parse::<Profile>().is_err());
    }
}
