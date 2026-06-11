//! System information collector — reads /proc and /sys for CPU, memory, network, disk.

use std::fs;

// ── Data structures ───────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct NetInterface {
    pub name: String,
    pub ip: String,
    pub rx_rate_mbps: f32,
    pub tx_rate_mbps: f32,
}

#[derive(Debug, Clone, Default)]
pub struct DiskMount {
    pub mount_point: String,
    pub total_gb: f32,
    pub used_gb: f32,
    pub percent: f32,
}

#[derive(Debug, Clone, Default)]
pub struct SystemInfo {
    // CPU
    pub cpu_percent: f32,
    pub cpu_model: String,
    pub cpu_count: u32,
    pub cpu_freq_mhz: f32,
    pub cpu_temp_c: f32,
    pub per_core_percents: Vec<f32>,
    pub load_avg_1: f32,
    pub load_avg_5: f32,
    pub load_avg_15: f32,
    // Memory
    pub mem_total_gb: f32,
    pub mem_used_gb: f32,
    pub mem_percent: f32,
    pub mem_cached_gb: f32,
    pub mem_buffers_gb: f32,
    pub swap_total_gb: f32,
    pub swap_used_gb: f32,
    pub swap_percent: f32,
    // Network
    pub net_interfaces: Vec<NetInterface>,
    // Disk
    pub disk_mounts: Vec<DiskMount>,
    // System
    pub hostname: String,
    pub kernel_version: String,
    pub uptime_seconds: u64,
    pub uptime_str: String,
    pub process_count: u32,
}

/// Samples needed for delta calculation across polls.
#[derive(Debug, Clone)]
pub struct CpuSamples {
    /// (idle, total) for aggregate CPU
    pub aggregate: (u64, u64),
    /// per-core (idle, total), one per logical core
    pub per_core: Vec<(u64, u64)>,
    /// previous network (name, rx_bytes, tx_bytes) for rate calculation
    pub net_prev: Vec<(String, u64, u64)>,
}

impl Default for CpuSamples {
    fn default() -> Self {
        Self { aggregate: (0, 0), per_core: Vec::new(), net_prev: Vec::new() }
    }
}

// ── CPU ────────────────────────────────────────────────────────────────

fn read_cpu_times(line: &str) -> Option<(u64, u64)> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.is_empty() { return None; }
    let nums: Vec<u64> = fields[1..].iter().filter_map(|s| s.parse().ok()).collect();
    if nums.len() < 4 { return None; }
    let idle = nums[3] + nums.get(4).copied().unwrap_or(0);
    let total: u64 = nums.iter().sum();
    Some((idle, total))
}

fn read_cpuinfo_model() -> String {
    let Ok(content) = fs::read_to_string("/proc/cpuinfo") else { return String::new(); };
    for line in content.lines() {
        if line.starts_with("model name") {
            if let Some(v) = line.split(':').nth(1) {
                return v.trim().to_string();
            }
        }
    }
    String::new()
}

fn read_cpu_freq() -> f32 {
    // Read from first core's scaling_cur_freq (in kHz)
    for path in &[
        "/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq",
        "/sys/devices/system/cpu/cpufreq/policy0/scaling_cur_freq",
    ] {
        if let Ok(s) = fs::read_to_string(path) {
            if let Ok(khz) = s.trim().parse::<f32>() {
                return khz / 1000.0; // kHz -> MHz
            }
        }
    }
    0.0
}

fn read_cpu_temp() -> f32 {
    for i in 0..10 {
        let path = format!("/sys/class/thermal/thermal_zone{i}/temp");
        if let Ok(s) = fs::read_to_string(&path) {
            // Check if this zone is CPU-related
            let type_path = format!("/sys/class/thermal/thermal_zone{i}/type");
            if let Ok(t) = fs::read_to_string(&type_path) {
                if t.trim().starts_with("x86_pkg") || t.trim().starts_with("cpu")
                    || t.trim().starts_with("acpitz")
                {
                    if let Ok(mc) = s.trim().parse::<f32>() {
                        return mc / 1000.0; // millidegree -> degree
                    }
                }
            }
        }
    }
    0.0
}

// ── Memory ─────────────────────────────────────────────────────────────

fn read_meminfo() -> Option<MemInfo> {
    let content = fs::read_to_string("/proc/meminfo").ok()?;
    let mut m = MemInfo::default();
    for line in content.lines() {
        match () {
            _ if line.starts_with("MemTotal:")     => m.total = parse_kb(line),
            _ if line.starts_with("MemAvailable:") => m.avail = parse_kb(line),
            _ if line.starts_with("Cached:")       => m.cached = parse_kb(line),
            _ if line.starts_with("Buffers:")      => m.buffers = parse_kb(line),
            _ if line.starts_with("SwapTotal:")    => m.swap_total = parse_kb(line),
            _ if line.starts_with("SwapFree:")     => m.swap_free = parse_kb(line),
            _ => {},
        }
    }
    if m.total == 0 { return None; }
    Some(m)
}

#[derive(Default)]
struct MemInfo {
    total: u64,
    avail: u64,
    cached: u64,
    buffers: u64,
    swap_total: u64,
    swap_free: u64,
}

fn parse_kb(line: &str) -> u64 {
    line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0)
}

// ── Network ────────────────────────────────────────────────────────────

fn read_net_stats() -> Vec<(String, u64, u64)> {
    let Ok(content) = fs::read_to_string("/proc/net/dev") else { return vec![]; };
    let mut out = Vec::new();
    for line in content.lines().skip(2) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 10 { continue; }
        let name = parts[0].trim_end_matches(':').to_string();
        // Skip loopback
        if name == "lo" { continue; }
        let rx: u64 = parts[1].parse().unwrap_or(0);
        let tx: u64 = parts[9].parse().unwrap_or(0);
        out.push((name, rx, tx));
    }
    out
}

fn read_ip_for(iface: &str) -> String {
    // Read IPv6 address from /proc/net/if_inet6
    if let Ok(content) = fs::read_to_string("/proc/net/if_inet6") {
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 6 && parts[5] == iface {
                let raw = parts[0];
                if raw.len() == 32 {
                    return (0..8).map(|i| &raw[i*4..i*4+4]).collect::<Vec<_>>().join(":");
                }
            }
        }
    }
    String::new()
}
// ── Disk ───────────────────────────────────────────────────────────────

fn read_disk_mounts() -> Vec<DiskMount> {
    let Ok(content) = fs::read_to_string("/proc/mounts") else { return vec![]; };
    let mut out = Vec::new();
    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 { continue; }
        let dev = parts[0];
        let mp = parts[1];
        // Only real filesystems on block devices
        if !dev.starts_with("/dev/") { continue; }
        // Skip pseudofs
        if mp.starts_with("/sys") || mp.starts_with("/proc") || mp.starts_with("/dev")
            || mp.starts_with("/run") || mp == "/snap" {
            continue;
        }
        out.push(DiskMount {
            mount_point: mp.to_string(),
            total_gb: 0.0,
            used_gb: 0.0,
            percent: 0.0,
        });
        if out.len() >= 4 { break; } // limit to top 4
    }
    out
}

// ── System ─────────────────────────────────────────────────────────────

fn read_hostname() -> String {
    fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn read_kernel() -> String {
    fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn count_processes() -> u32 {
    if let Ok(entries) = fs::read_dir("/proc") {
        entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .chars()
                    .all(|c| c.is_ascii_digit())
            })
            .count() as u32
    } else {
        0
    }
}

fn read_uptime() -> Option<f64> {
    fs::read_to_string("/proc/uptime")
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

fn read_loadavg() -> Option<(f32, f32, f32)> {
    let content = fs::read_to_string("/proc/loadavg").ok()?;
    let parts: Vec<&str> = content.split_whitespace().collect();
    if parts.len() < 3 { return None; }
    Some((parts[0].parse().ok()?, parts[1].parse().ok()?, parts[2].parse().ok()?))
}

fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let mins = (seconds % 3600) / 60;
    if days > 0 {
        format!("{days}d {hours}h {mins}m")
    } else if hours > 0 {
        format!("{hours}h {mins}m")
    } else {
        format!("{mins}m")
    }
}

fn cpu_percent_from_delta(prev: (u64, u64), cur: (u64, u64)) -> f32 {
    let idle_delta = cur.0.saturating_sub(prev.0);
    let total_delta = cur.1.saturating_sub(prev.1);
    if total_delta > 0 {
        100.0 * (1.0 - idle_delta as f32 / total_delta as f32)
    } else {
        0.0
    }
}

// ── Main collection ───────────────────────────────────────────────────

pub fn collect(info: &mut SystemInfo, prev: &CpuSamples) -> CpuSamples {
    let mut new_samples = CpuSamples::default();
    let mut out = SystemInfo::default();

    // ── CPU aggregate + per-core ───────────────────────────────────────
    if let Ok(stat) = fs::read_to_string("/proc/stat") {
        for line in stat.lines() {
            if line == "cpu" || line.starts_with("cpu ") {
                if let Some(s) = read_cpu_times(line) {
                    new_samples.aggregate = s;
                }
            } else if line.starts_with("cpu") {
                // per-core: cpu0, cpu1, ...
                if let Some(s) = read_cpu_times(line) {
                    new_samples.per_core.push(s);
                }
            }
        }
    }
    out.cpu_percent = cpu_percent_from_delta(prev.aggregate, new_samples.aggregate);
    for (i, cur) in new_samples.per_core.iter().enumerate() {
        let prev_core = prev.per_core.get(i).copied().unwrap_or((0, 0));
        out.per_core_percents.push(cpu_percent_from_delta(prev_core, *cur));
    }
    out.cpu_count = new_samples.per_core.len().max(1) as u32;
    out.cpu_model = read_cpuinfo_model();
    out.cpu_freq_mhz = read_cpu_freq();
    out.cpu_temp_c = read_cpu_temp();

    // ── Memory ─────────────────────────────────────────────────────────
    if let Some(m) = read_meminfo() {
        let used = m.total.saturating_sub(m.avail);
        out.mem_total_gb = m.total as f32 / 1_048_576.0;
        out.mem_used_gb = used as f32 / 1_048_576.0;
        out.mem_cached_gb = m.cached as f32 / 1_048_576.0;
        out.mem_buffers_gb = m.buffers as f32 / 1_048_576.0;
        if m.total > 0 {
            out.mem_percent = 100.0 * used as f32 / m.total as f32;
        }
        out.swap_total_gb = m.swap_total as f32 / 1_048_576.0;
        let swap_used = m.swap_total.saturating_sub(m.swap_free);
        out.swap_used_gb = swap_used as f32 / 1_048_576.0;
        if m.swap_total > 0 {
            out.swap_percent = 100.0 * swap_used as f32 / m.swap_total as f32;
        }
    }

    // ── Network ────────────────────────────────────────────────────────
    let net_now = read_net_stats();
    for (name, rx, tx) in &net_now {
        let prev_entry = prev.net_prev.iter().find(|(n, _, _)| n == name);
        let (rx_rate, tx_rate) = if let Some((_, prx, ptx)) = prev_entry {
            let drx = rx.saturating_sub(*prx) as f32;
            let dtx = tx.saturating_sub(*ptx) as f32;
            // bytes per 2s → Mbps
            (drx * 8.0 / 2.0 / 1_000_000.0, dtx * 8.0 / 2.0 / 1_000_000.0)
        } else {
            (0.0, 0.0)
        };
        let ip = read_ip_for(name);
        out.net_interfaces.push(NetInterface {
            name: name.clone(),
            ip,
            rx_rate_mbps: rx_rate,
            tx_rate_mbps: tx_rate,
        });
    }
    new_samples.net_prev = net_now;

    // ── Disk ───────────────────────────────────────────────────────────
    out.disk_mounts = read_disk_mounts();

    // ── System ─────────────────────────────────────────────────────────
    out.hostname = read_hostname();
    out.kernel_version = read_kernel();
    out.process_count = count_processes();
    if let Some(up) = read_uptime() {
        out.uptime_seconds = up as u64;
        out.uptime_str = format_uptime(up as u64);
    }
    if let Some((l1, l5, l15)) = read_loadavg() {
        out.load_avg_1 = l1;
        out.load_avg_5 = l5;
        out.load_avg_15 = l15;
    }

    *info = out;
    new_samples
}
