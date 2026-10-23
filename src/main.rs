use clap::{Parser, Subcommand};
use hardness_gfx::cache;
use hardness_gfx::doctor;
use hardness_gfx::host;
use hardness_gfx::logs::SoberLog;
use hardness_gfx::profile::Profile;
use hardness_gfx::run;
use hardness_gfx::sober;
use hardness_gfx::vulkan;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "hardness-gfx",
    version,
    about = "Diagnostico e validacao do caminho grafico Android/Vulkan do Sober"
)]
struct Cli {
    /// Diretorio de relatorios e logs das execucoes.
    #[arg(long, default_value = "artifacts/reports", global = true)]
    output: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Coleta host, Sober, config, log, cache e dispositivos Vulkan.
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Lista variantes observadas nos logs do Sober.
    Variants {
        #[arg(long)]
        json: bool,
    },
    /// Executa uma sonda Vulkan/WSI independente.
    Probe {
        #[arg(long)]
        json: bool,
    },
    /// Inspeciona o cache de pipeline Vulkan do Sober.
    Cache {
        #[arg(long)]
        json: bool,
    },
    /// Executa o Sober com um perfil e persiste um relatorio de validacao.
    Verify {
        #[arg(long, default_value = "baseline")]
        profile: String,
        #[arg(long, default_value = "launch-controlled")]
        scenario: String,
        #[arg(long)]
        json: bool,
    },
    /// Executa o Sober com um perfil, sem gerar relatorio estruturado.
    Run {
        #[arg(long, default_value = "baseline")]
        profile: String,
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
}

#[derive(Debug, Serialize)]
struct Variant {
    key: String,
    runs: usize,
    present_modes: BTreeSet<String>,
    formats: BTreeSet<String>,
    resolutions: BTreeSet<String>,
    create_vulkan: BTreeSet<String>,
    pipeline_cache_skips: usize,
}

fn main() {
    if let Err(error) = execute() {
        eprintln!("error: {error}");
        std::process::exit(2);
    }
}

fn execute() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Doctor { json } => {
            let report = doctor::collect();
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_doctor(&report);
            }
        }
        Commands::Variants { json } => {
            let install = sober::collect();
            let variants = collect_variants(&install);
            if json {
                println!("{}", serde_json::to_string_pretty(&variants)?);
            } else {
                print_variants(&variants);
            }
        }
        Commands::Probe { json } => {
            let probe = vulkan::probe();
            if json {
                println!("{}", serde_json::to_string_pretty(&probe)?);
            } else {
                print_probe(&probe);
            }
        }
        Commands::Cache { json } => {
            let install = sober::collect();
            let host = host::collect();
            let (vendor, device) = gpu_ids(&host);
            let path = install.user_data.join("cache/sober/shadercachevk.bin");
            let inspected = cache::inspect(&path, vendor, device)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&inspected)?);
            } else {
                print_cache(inspected.as_ref());
            }
        }
        Commands::Verify {
            profile,
            scenario,
            json,
        } => {
            let profile = parse_profile(&profile)?;
            let report = doctor::verify(profile, &scenario, &cli.output)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_verify(&report);
            }
            if report.checks.iter().any(|c| c.status == "FAIL") {
                std::process::exit(1);
            }
        }
        Commands::Run { profile, args } => {
            let profile = parse_profile(&profile)?;
            let output = run::launch(profile, &args, &cli.output.join("logs"))?;
            println!("command: {}", output.command);
            println!("log: {}", output.log_path.display());
            println!("stderr: {}", output.stderr_path.display());
            println!("exit: {:?}", output.exit_code);
            if !output.success {
                std::process::exit(output.exit_code.unwrap_or(1));
            }
        }
    }
    Ok(())
}

fn parse_profile(value: &str) -> Result<Profile, String> {
    value.parse()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser as _;

    #[test]
    fn cli_accepts_run_and_verify_invocations() {
        // Debug asserts in clap reject invalid combinations (e.g. `last` +
        // `trailing_var_arg`) only when parsing is attempted; this keeps the
        // contract for both launch subcommands under cargo test.
        assert!(Cli::try_parse_from(["rea-gfx", "run", "--profile", "baseline"]).is_ok());
        assert!(Cli::try_parse_from([
            "rea-gfx",
            "verify",
            "--profile",
            "fifo",
            "--scenario",
            "test"
        ])
        .is_ok());
    }
}

fn gpu_ids(host: &host::HostInfo) -> (Option<u32>, Option<u32>) {
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

fn print_doctor(report: &doctor::DoctorReport) {
    println!("Hardness GFX Doctor");
    println!("gerado: {}", report.generated_at);
    println!(
        "host: {} | kernel {} | sessao {:?}",
        report.host.os.as_deref().unwrap_or("desconhecido"),
        report.host.kernel.as_deref().unwrap_or("desconhecido"),
        report.host.session_type
    );
    for gpu in &report.host.gpus {
        println!(
            "gpu: {} | pci {:?} | vendor {:?} | device {:?} | driver {:?}",
            gpu.card, gpu.pci_id, gpu.vendor_id, gpu.device_id, gpu.driver
        );
    }
    if let Some(mesa) = &report.host.mesa {
        println!(
            "mesa: renderer={:?} version={:?}",
            mesa.renderer, mesa.version
        );
    }
    println!(
        "sober: {:?} | commit {:?} | config {}",
        report.sober.version,
        report.sober.commit,
        report.sober.config_path.display()
    );
    if let Some(config) = &report.config {
        println!(
            "config: use_opengl={:?} modo={:?} gamemode={:?} hidpi={:?} fflags={}",
            config.use_opengl,
            config.graphics_optimization_mode,
            config.enable_gamemode,
            config.enable_hidpi,
            config.fflags.len()
        );
    }
    if let Some(log) = &report.latest_log {
        println!(
            "log: {} | vulkan={:?} opengl={:?} api={:?} mesa={:?}",
            log.path.display(),
            log.create_vulkan,
            log.create_opengl,
            log.vulkan_api,
            log.mesa
        );
        println!(
            "swapchain: {:?} | present={:?} | format={:?} | shaders={:?}",
            log.resolution, log.present_mode, log.format, log.shader_count
        );
        if log.pipeline_cache_skipped {
            println!("aviso: pipeline cache foi ignorado em uma execucao");
        }
    }
    println!("vulkan: {} dispositivo(s)", report.vulkan.devices.len());
    for device in &report.vulkan.devices {
        println!(
            "  - {} | {:?} | api={:?} driver={:?} heap={:?}",
            device.name,
            device.device_type,
            device.api_version,
            device.driver_version,
            device.heaps.first().map(|h| h.size)
        );
        if let Some(surface) = &device.surface {
            println!(
                "    surface: {} format={:?} present={:?} extent={:?}",
                surface.backend,
                surface.chosen_format,
                surface.present_modes,
                surface.current_extent
            );
        } else if let Some(error) = &device.surface_error {
            println!("    surface indisponivel: {error}");
        }
    }
    println!("checks:");
    for check in &report.checks {
        println!("  [{}] {}: {}", check.status, check.id, check.detail);
    }
    println!("findings:");
    for finding in &report.findings {
        println!(
            "  [{}:{}] {} — {}",
            finding.level, finding.kind, finding.id, finding.statement
        );
    }
}

fn collect_variants(install: &sober::SoberInstall) -> Vec<Variant> {
    let mut map: BTreeMap<String, Variant> = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(&install.logs_dir) else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("log") {
            continue;
        }
        let Ok(log) = SoberLog::read(&path) else {
            continue;
        };
        let key = format!(
            "gpu={:?} mesa={:?} api={:?} driver={:?}",
            log.gpu, log.mesa, log.vulkan_api, log.driver_version
        );
        let variant = map.entry(key.clone()).or_insert_with(|| Variant {
            key,
            runs: 0,
            present_modes: BTreeSet::new(),
            formats: BTreeSet::new(),
            resolutions: BTreeSet::new(),
            create_vulkan: BTreeSet::new(),
            pipeline_cache_skips: 0,
        });
        variant.runs += 1;
        if let Some(v) = log.present_mode {
            variant.present_modes.insert(v.to_string());
        }
        if let Some(v) = log.format {
            variant.formats.insert(v.to_string());
        }
        if let Some(v) = log.resolution {
            variant.resolutions.insert(v);
        }
        variant
            .create_vulkan
            .insert(format!("{:?}", log.create_vulkan));
        if log.pipeline_cache_skipped {
            variant.pipeline_cache_skips += 1;
        }
    }
    map.into_values().collect()
}

fn print_variants(variants: &[Variant]) {
    if variants.is_empty() {
        println!("nenhum log encontrado");
        return;
    }
    for variant in variants {
        println!(
            "{} | runs={} | present={:?} | format={:?} | extent={:?} | vulkan={:?} | cache_skips={}",
            variant.key,
            variant.runs,
            variant.present_modes,
            variant.formats,
            variant.resolutions,
            variant.create_vulkan,
            variant.pipeline_cache_skips
        );
    }
}

fn print_probe(probe: &vulkan::VulkanProbe) {
    println!(
        "loader: {:?} | instance={:?} | extensions={}",
        probe.loader,
        probe.instance_version,
        probe.instance_extensions.len()
    );
    if let Some(error) = &probe.error {
        println!("error: {error}");
    }
    for device in &probe.devices {
        println!(
            "device: {} | {} | vendor=0x{:04x} device=0x{:04x} | api={:?} driver={:?} | unified={}",
            device.name,
            device.device_type,
            device.vendor_id,
            device.device_id,
            device.api_version,
            device.driver_version,
            device.unified_memory
        );
        for heap in &device.heaps {
            println!(
                "  heap {}: {} bytes device_local={} host_visible={}",
                heap.index, heap.size, heap.device_local, heap.host_visible
            );
        }
        if let Some(surface) = &device.surface {
            println!(
                "  surface {}: format={:?} present={:?} extent={:?} images={:?}..{:?}",
                surface.backend,
                surface.chosen_format,
                surface.present_modes,
                surface.current_extent,
                surface.min_image_count,
                surface.max_image_count
            );
        } else if let Some(error) = &device.surface_error {
            println!("  surface indisponivel: {error}");
        }
    }
}

fn print_cache(cache: Option<&cache::PipelineCache>) {
    match cache {
        Some(cache) => {
            println!(
                "cache: {} | {} bytes | magic={:?} | header_offset={:?}",
                cache.path.display(),
                cache.file_size,
                cache.magic,
                cache.header_offset
            );
            println!(
                "header: size={:?} version={:?} vendor={:?} device={:?} uuid={:?}",
                cache.header_size,
                cache.header_version,
                cache.vendor_id.map(|v| format!("0x{v:04x}")),
                cache.device_id.map(|v| format!("0x{v:04x}")),
                cache.uuid
            );
            println!(
                "valid={} device_match={:?}",
                cache.format_valid, cache.matches_expected_device
            );
        }
        None => println!("cache ausente"),
    }
}

fn print_verify(report: &doctor::VerifyReport) {
    println!("Hardness GFX Verify");
    println!("perfil: {:?}", report.profile);
    println!("cenario: {}", report.scenario);
    println!("command: {}", report.run.command);
    println!("log: {}", report.run.log_path.display());
    println!("stderr: {}", report.run.stderr_path.display());
    println!("exit: {:?}", report.run.exit_code);
    if let Some(report_path) = &report.report_path {
        println!("report: {}", report_path.display());
    }
    println!("checks:");
    for check in &report.checks {
        println!("  [{}] {}: {}", check.status, check.id, check.detail);
    }
    println!("findings:");
    for finding in &report.findings {
        println!(
            "  [{}:{}] {} — {}",
            finding.level, finding.kind, finding.id, finding.statement
        );
    }
}
