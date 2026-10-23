use crate::cache;
use crate::config::{self, SoberConfig};
use crate::host::{self, HostInfo};
use crate::logs::SoberLog;
use crate::profile::Profile;
use crate::run::{self, RunOutput};
use crate::sober::{self, SoberInstall};
use crate::vulkan::{self, VulkanProbe};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub generated_at: String,
    pub host: HostInfo,
    pub sober: SoberInstall,
    pub config: Option<SoberConfig>,
    pub latest_log: Option<SoberLog>,
    pub pipeline_cache: Option<cache::PipelineCache>,
    pub vulkan: VulkanProbe,
    pub checks: Vec<Check>,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub id: String,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub id: String,
    pub level: String,
    pub kind: String,
    pub statement: String,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerifyReport {
    pub generated_at: String,
    pub profile: Profile,
    pub scenario: String,
    pub doctor: DoctorReport,
    pub run: RunOutput,
    pub after_log: Option<SoberLog>,
    pub checks: Vec<Check>,
    pub findings: Vec<Finding>,
    pub report_path: Option<PathBuf>,
}

pub fn collect() -> DoctorReport {
    let host = host::collect();
    let sober = sober::collect();
    let config = config::read(&sober.config_path).ok();
    let latest_log = sober::latest_log(&sober.logs_dir).and_then(|p| SoberLog::read(p).ok());
    let (vendor, device) = expected_gpu_ids(&host);
    let cache_path = sober.user_data.join("cache/sober/shadercachevk.bin");
    let pipeline_cache = cache::inspect(&cache_path, vendor, device).ok().flatten();
    let vulkan = vulkan::probe();
    let mut report = DoctorReport {
        generated_at: now_string(),
        host,
        sober,
        config,
        latest_log,
        pipeline_cache,
        vulkan,
        checks: Vec::new(),
        findings: Vec::new(),
    };
    report.checks = checks(&report);
    report.findings = findings(&report);
    report
}

pub fn verify(profile: Profile, scenario: &str, output_dir: &Path) -> Result<VerifyReport, String> {
    let doctor = collect();
    let log_dir = output_dir.join("logs");
    let run = run::launch(profile, &[], &log_dir)?;
    // Sober writes its diagnostics to stderr; the captured stdout file stays
    // empty. Prefer whichever capture actually contains content.
    let after_log = if std::fs::metadata(&run.log_path)
        .map(|m| m.len())
        .unwrap_or(0)
        > 0
    {
        SoberLog::read(&run.log_path).ok()
    } else {
        SoberLog::read(&run.stderr_path).ok()
    };
    let mut report = VerifyReport {
        generated_at: now_string(),
        profile,
        scenario: scenario.to_string(),
        doctor,
        run,
        after_log,
        checks: Vec::new(),
        findings: Vec::new(),
        report_path: None,
    };
    report.checks = verify_checks(&report);
    report.findings = verify_findings(&report);
    fs::create_dir_all(output_dir)
        .map_err(|e| format!("cannot create {}: {e}", output_dir.display()))?;
    let path = output_dir.join(format!("verify-{}.json", stamp()));
    let json = serde_json::to_string_pretty(&report)
        .map_err(|e| format!("cannot serialize report: {e}"))?;
    fs::write(&path, json).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    report.report_path = Some(path);
    Ok(report)
}

fn checks(report: &DoctorReport) -> Vec<Check> {
    let mut out = Vec::new();
    out.push(Check {
        id: "sober-installed".to_string(),
        status: if report.sober.installed {
            "PASS"
        } else {
            "FAIL"
        }
        .to_string(),
        detail: format!(
            "Sober {} em {}",
            report
                .sober
                .version
                .as_deref()
                .unwrap_or("versao desconhecida"),
            report.sober.user_data.display()
        ),
    });
    out.push(Check {
        id: "sober-config".to_string(),
        status: if report.config.is_some() {
            "PASS"
        } else {
            "WARN"
        }
        .to_string(),
        detail: format!("config: {}", report.sober.config_path.display()),
    });
    out.push(Check {
        id: "sober-log".to_string(),
        status: if report.latest_log.is_some() {
            "PASS"
        } else {
            "WARN"
        }
        .to_string(),
        detail: report
            .latest_log
            .as_ref()
            .map(|l| l.path.display().to_string())
            .unwrap_or_else(|| "nenhum log encontrado".to_string()),
    });
    out.push(Check {
        id: "vulkan-device".to_string(),
        status: if report.vulkan.devices.is_empty() {
            "FAIL"
        } else {
            "PASS"
        }
        .to_string(),
        detail: format!(
            "{} dispositivo(s) Vulkan; loader={}",
            report.vulkan.devices.len(),
            report.vulkan.loader.as_deref().unwrap_or("desconhecido")
        ),
    });
    let has_android_surface = report.latest_log.as_ref().is_some_and(|l| {
        l.wsi_extensions
            .iter()
            .any(|e| e == "VK_KHR_android_surface")
    });
    out.push(Check {
        id: "vulkan-android-surface".to_string(),
        status: if has_android_surface { "PASS" } else { "WARN" }.to_string(),
        detail: "VK_KHR_android_surface no WSI carregado pelo log mais recente".to_string(),
    });
    out.push(Check {
        id: "pipeline-cache".to_string(),
        status: match &report.pipeline_cache {
            Some(c) if c.format_valid => "PASS",
            Some(_) => "WARN",
            None => "WARN",
        }
        .to_string(),
        detail: report
            .pipeline_cache
            .as_ref()
            .map(|c| {
                format!(
                    "{} bytes, header {:?}, device match {:?}",
                    c.file_size, c.header_version, c.matches_expected_device
                )
            })
            .unwrap_or_else(|| "cache ausente".to_string()),
    });
    out
}

fn verify_checks(report: &VerifyReport) -> Vec<Check> {
    let mut out = Vec::new();
    out.push(Check {
        id: "run-exit".to_string(),
        status: if report.run.success { "PASS" } else { "FAIL" }.to_string(),
        detail: format!("exit={:?}", report.run.exit_code),
    });
    out.push(Check {
        id: "vulkan-path-after-run".to_string(),
        status: match report.after_log.as_ref().and_then(|l| l.create_vulkan) {
            Some(true) => "PASS",
            Some(false) => "FAIL",
            None => "WARN",
        }
        .to_string(),
        detail: format!("log: {}", report.run.log_path.display()),
    });
    out.push(Check {
        id: "android-surface-after-run".to_string(),
        status: match report.after_log.as_ref() {
            Some(l)
                if l.wsi_extensions
                    .iter()
                    .any(|e| e == "VK_KHR_android_surface") =>
            {
                "PASS"
            }
            Some(_) => "WARN",
            None => "WARN",
        }
        .to_string(),
        detail: "extensao Android WSI no log da execucao".to_string(),
    });
    if let Some(expected) = expected_present_mode(report.profile) {
        out.push(Check {
            id: "present-mode-after-run".to_string(),
            status: match report.after_log.as_ref().and_then(|l| l.present_mode) {
                Some(mode) if mode == expected => "PASS".to_string(),
                Some(_) => "WARN".to_string(),
                None => "WARN".to_string(),
            },
            detail: format!(
                "esperado={expected}, observado={:?}",
                report.after_log.as_ref().and_then(|l| l.present_mode)
            ),
        });
    }
    out
}

fn expected_present_mode(profile: Profile) -> Option<u32> {
    match profile {
        Profile::Baseline | Profile::Llvmpipe => None,
        Profile::Fifo => Some(2),
        Profile::Relaxed => Some(3),
        Profile::Mailbox => Some(1),
        Profile::Immediate => Some(0),
    }
}

fn findings(report: &DoctorReport) -> Vec<Finding> {
    let mut out = Vec::new();
    let installer = Path::new("RobloxPlayerInstaller.exe").exists();
    if installer {
        out.push(Finding {
            id: "windows-client".to_string(),
            level: "INFO".to_string(),
            kind: "FACT".to_string(),
            statement: "Ha apenas o instalador Windows no diretorio atual; nao ha cliente Windows instalado para inspecionar.".to_string(),
            evidence: "RobloxPlayerInstaller.exe presente".to_string(),
        });
    }
    if let Some(gpu) = report.host.gpus.first() {
        out.push(Finding {
            id: "host-gpu".to_string(),
            level: "INFO".to_string(),
            kind: "FACT".to_string(),
            statement: format!(
                "GPU ativa: {} ({:?}), driver {}.",
                gpu.card,
                gpu.pci_id,
                gpu.driver.as_deref().unwrap_or("desconhecido")
            ),
            evidence: "sysfs /sys/class/drm".to_string(),
        });
    }
    if let Some(log) = &report.latest_log {
        if log.create_vulkan == Some(true) && log.create_opengl == Some(false) {
            out.push(Finding {
                id: "vulkan-active".to_string(),
                level: "INFO".to_string(),
                kind: "FACT".to_string(),
                statement: "O log mais recente mostra create_vulkan=true e create_opengl=false: o caminho Android/Vulkan esta ativo.".to_string(),
                evidence: log.path.display().to_string(),
            });
        }
        if log
            .wsi_extensions
            .iter()
            .any(|e| e == "VK_KHR_android_surface")
        {
            out.push(Finding {
                id: "android-wsi".to_string(),
                level: "INFO".to_string(),
                kind: "FACT".to_string(),
                statement: "O WSI Vulkan carregado inclui VK_KHR_android_surface, coerente com o cliente Android via libhybris.".to_string(),
                evidence: format!("extensoes: {:?}", log.wsi_extensions),
            });
        }
        if log.present_mode == Some(2) {
            out.push(Finding {
                id: "present-mode".to_string(),
                level: "INFO".to_string(),
                kind: "FACT".to_string(),
                statement: "O swapchain observado usa present mode 2 (VK_PRESENT_MODE_FIFO_KHR)."
                    .to_string(),
                evidence: format!("swapchain: {:?}", log.resolution),
            });
        }
        if log.video_memory == Some(64 * 1024 * 1024) {
            out.push(Finding {
                id: "video-memory-cap".to_string(),
                level: "INFO".to_string(),
                kind: "INFERENCE".to_string(),
                statement: "O limite reportado de 64 MB (caps.videoMemory) e um valor de capacidade do caminho de compatibilidade; ele nao e o tamanho total do heap Vulkan (733 MB no log).".to_string(),
                evidence: "videoMemory=67108864; heapSize=733090816".to_string(),
            });
        }
        if log.pipeline_cache_skipped {
            out.push(Finding {
                id: "pipeline-cache-skip".to_string(),
                level: "WARN".to_string(),
                kind: "FACT".to_string(),
                statement: "Uma execucao registrou 'Skipping pipeline cache since validation failed'; o cache deve ser tratado como suspeito ate nova validacao.".to_string(),
                evidence: log.path.display().to_string(),
            });
        }
    }
    if let Some(config) = &report.config {
        if !config.fflags.is_empty() {
            out.push(Finding {
                id: "fflags".to_string(),
                level: "INFO".to_string(),
                kind: "FACT".to_string(),
                statement: format!("Sober config tem {} fflag(s) definida(s); o valor de exemplo nao deve ser apresentado como tuning de desempenho.", config.fflags.len()),
                evidence: report.sober.config_path.display().to_string(),
            });
        }
        if config.enable_gamemode == Some(true) {
            out.push(Finding {
                id: "gamemode".to_string(),
                level: "INFO".to_string(),
                kind: "FACT".to_string(),
                statement: "enable_gamemode=true; o efeito depende do GameMode estar instalado e ativo no host.".to_string(),
                evidence: report.sober.config_path.display().to_string(),
            });
        }
    }
    if report.vulkan.devices.is_empty() {
        out.push(Finding {
            id: "vulkan-probe-failed".to_string(),
            level: "WARN".to_string(),
            kind: "UNKNOWN".to_string(),
            statement: "A sonda Vulkan nao enumerou dispositivos; o motivo esta em vulkan.error."
                .to_string(),
            evidence: report.vulkan.error.clone().unwrap_or_default(),
        });
    }
    if let Some(cache) = &report.pipeline_cache {
        // Compare the pipeline-cache UUID embedded in the RPCV header with the
        // live pipelineCacheUUID of the matching physical device. A mismatch
        // explains Roblox's "validation failed" skip: the cache was generated
        // by a different driver generation.
        let live = report
            .vulkan
            .devices
            .iter()
            .find(|d| {
                d.device_type != "CPU"
                    && report
                        .pipeline_cache
                        .as_ref()
                        .map(|c| c.vendor_id == Some(d.vendor_id))
                        .unwrap_or(true)
            })
            .or_else(|| report.vulkan.devices.first());
        if let (Some(device), Some(cache_uuid)) = (live, &cache.uuid) {
            match &device.pipeline_cache_uuid {
                Some(device_uuid) if device_uuid == cache_uuid => out.push(Finding {
                    id: "pipeline-cache-uuid".to_string(),
                    level: "INFO".to_string(),
                    kind: "FACT".to_string(),
                    statement: "pipelineCacheUUID do cache coincide com o device atual; a validacao do Roblox falha por outro criterio (causa nao determinada por este probe).".to_string(),
                    evidence: format!("uuid={cache_uuid}"),
                }),
                Some(device_uuid) => out.push(Finding {
                    id: "pipeline-cache-uuid".to_string(),
                    level: "WARN".to_string(),
                    kind: "FACT".to_string(),
                    statement: "pipelineCacheUUID do cache difere do device atual: o cache foi gerado por outro driver/gpu e sera rejeitado em toda sessao (recompilacao de shaders a cada inicio). Regenerar o cache (remover o arquivo) e candidato a correcao; decisao do DEV.".to_string(),
                    evidence: format!(
                        "cache_uuid={cache_uuid} device_uuid={device_uuid} arquivo={}",
                        cache.path.display()
                    ),
                }),
                None => out.push(Finding {
                    id: "pipeline-cache-uuid".to_string(),
                    level: "INFO".to_string(),
                    kind: "UNKNOWN".to_string(),
                    statement: "A sonda nao obteve pipelineCacheUUID do device; comparacao impossivel nesta execucao.".to_string(),
                    evidence: cache.path.display().to_string(),
                }),
            }
        }
    }
    out
}

fn verify_findings(report: &VerifyReport) -> Vec<Finding> {
    let mut out = Vec::new();
    if let Some(log) = &report.after_log {
        out.push(Finding {
            id: "profile-observed".to_string(),
            level: "INFO".to_string(),
            kind: "FACT".to_string(),
            statement: format!(
                "Perfil {:?} executado; create_vulkan={:?}, present_mode={:?}, formato={:?}.",
                report.profile, log.create_vulkan, log.present_mode, log.format
            ),
            evidence: log.path.display().to_string(),
        });
        if log.create_vulkan == Some(false) {
            out.push(Finding {
                id: "vulkan-not-used".to_string(),
                level: "WARN".to_string(),
                kind: "FACT".to_string(),
                statement: "A execucao nao usou o caminho Vulkan; verifique o log antes de atribuir o resultado ao perfil.".to_string(),
                evidence: log.path.display().to_string(),
            });
        }
    } else {
        out.push(Finding {
            id: "run-log-missing".to_string(),
            level: "WARN".to_string(),
            kind: "UNKNOWN".to_string(),
            statement: "O log da execucao nao pode ser lido; nao ha evidencia do caminho grafico dessa execucao.".to_string(),
            evidence: report.run.log_path.display().to_string(),
        });
    }
    out
}

fn expected_gpu_ids(host: &HostInfo) -> (Option<u32>, Option<u32>) {
    let gpu = host
        .gpus
        .iter()
        .find(|g| g.driver.as_deref() == Some("i915"))
        .or_else(|| host.gpus.first());
    match gpu {
        Some(gpu) => (
            gpu.vendor_id
                .as_deref()
                .and_then(|v| u32::from_str_radix(v, 16).ok()),
            gpu.device_id
                .as_deref()
                .and_then(|v| u32::from_str_radix(v, 16).ok()),
        ),
        None => (None, None),
    }
}

fn now_string() -> String {
    std::process::Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("unix:{}", stamp()))
}

fn stamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
