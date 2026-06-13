use std::sync::{Mutex, OnceLock};

use sysinfo::{Disks, Networks, System};

#[derive(Debug, Clone, Default)]
pub struct NetInterface {
  pub name: String,
  pub ip: String,
  pub rx_rate_mbps: f32,
  pub tx_rate_mbps: f32,
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct DiskMount {
  pub mount_point: String,
  pub total_gb: f32,
  pub used_gb: f32,
  pub percent: f32,
}

#[derive(Debug, Clone, Default)]
pub struct SystemInfo {
  pub cpu_percent: f32,
  pub cpu_model: String,
  pub cpu_count: u32,
  pub cpu_freq_mhz: f32,
  pub cpu_temp_c: f32,
  pub per_core_percents: Vec<f32>,
  pub load_avg_1: f32,
  pub load_avg_5: f32,
  pub load_avg_15: f32,
  pub mem_total_gb: f32,
  pub mem_used_gb: f32,
  pub mem_percent: f32,
  pub mem_cached_gb: f32,
  pub mem_buffers_gb: f32,
  pub swap_total_gb: f32,
  pub swap_used_gb: f32,
  pub swap_percent: f32,
  pub net_interfaces: Vec<NetInterface>,
  pub disk_mounts: Vec<DiskMount>,
  pub hostname: String,
  pub kernel_version: String,
  pub uptime_seconds: u64,
  pub uptime_str: String,
  pub process_count: u32,
}

#[derive(Debug, Clone, Default)]
pub struct CpuSamples {
  pub net_prev: Vec<(String, u64, u64)>,
}

const POLL_INTERVAL_SECS: f32 = 2.0;
const BYTES_PER_GIB: f32 = 1_073_741_824.0;

static SYS: OnceLock<Mutex<System>> = OnceLock::new();
static NETS: OnceLock<Mutex<Networks>> = OnceLock::new();

fn local_ip() -> String {
  std::net::UdpSocket::bind("0.0.0.0:0")
    .and_then(|s| {
      s.connect("8.8.8.8:80")?;
      s.local_addr().map(|a| a.ip().to_string())
    })
    .unwrap_or_default()
}

fn is_virtual_nic(name: &str) -> bool {
  let n = name.to_lowercase();
  n == "lo" || n == "lo0"
    || n.starts_with("loopback pseudo-interface")
    || n.starts_with("docker")
    || n.starts_with("veth")
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

pub fn collect(info: &mut SystemInfo, prev: &CpuSamples) -> CpuSamples {
  let mut new_samples = CpuSamples::default();
  let mut out = SystemInfo::default();

  let mut sys = SYS.get_or_init(|| Mutex::new(System::new())).lock().unwrap();
  let mut nets = NETS
    .get_or_init(|| Mutex::new(Networks::new_with_refreshed_list()))
    .lock()
    .unwrap();

  sys.refresh_cpu_usage();
  sys.refresh_memory();

  out.cpu_percent = sys.global_cpu_usage();
  out.cpu_count = sys.cpus().len().max(1) as u32;
  if let Some(cpu) = sys.cpus().first() {
    out.cpu_model = cpu.brand().to_string();
    out.cpu_freq_mhz = cpu.frequency() as f32;
  }
  for cpu in sys.cpus() {
    out.per_core_percents.push(cpu.cpu_usage());
  }

  #[cfg(target_os = "linux")]
  {
    let components = sysinfo::Components::new_with_refreshed_list();
    for comp in components.list() {
      let label = comp.label().to_lowercase();
      if label.contains("cpu") || label.contains("core") || label.contains("package") {
        out.cpu_temp_c = comp.temperature();
        break;
      }
    }
  }

  let total = sys.total_memory();
  let used = sys.used_memory();
  out.mem_total_gb = total as f32 / BYTES_PER_GIB;
  out.mem_used_gb = used as f32 / BYTES_PER_GIB;
  out.mem_cached_gb = 0.0;
  out.mem_buffers_gb = 0.0;
  #[cfg(target_os = "linux")]
  {
    if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
      for line in content.lines() {
        let mut parts = line.split_whitespace();
        match parts.next() {
          Some("Cached:") => {
            if let Some(kb) = parts.next().and_then(|s| s.parse::<u64>().ok()) {
              out.mem_cached_gb = kb as f32 * 1024.0 / BYTES_PER_GIB;
            }
          }
          Some("Buffers:") => {
            if let Some(kb) = parts.next().and_then(|s| s.parse::<u64>().ok()) {
              out.mem_buffers_gb = kb as f32 * 1024.0 / BYTES_PER_GIB;
            }
          }
          _ => {}
        }
      }
    }
  }
  if total > 0 {
    out.mem_percent = 100.0 * used as f32 / total as f32;
  }
  let swap_total = sys.total_swap();
  let swap_used = sys.used_swap();
  out.swap_total_gb = swap_total as f32 / BYTES_PER_GIB;
  out.swap_used_gb = swap_used as f32 / BYTES_PER_GIB;
  if swap_total > 0 {
    out.swap_percent = 100.0 * swap_used as f32 / swap_total as f32;
  }

  nets.refresh();
  let ip = local_ip();
  let mut first_iface = true;
  for (name, data) in nets.list() {
    if is_virtual_nic(name) {
      continue;
    }
    let rx = data.received();
    let tx = data.transmitted();
    let prev_entry = prev.net_prev.iter().find(|(n, _, _)| n == name);
    let (rx_rate, tx_rate) = if let Some((_, prx, ptx)) = prev_entry {
      let drx = rx.saturating_sub(*prx) as f32;
      let dtx = tx.saturating_sub(*ptx) as f32;
      (drx * 8.0 / POLL_INTERVAL_SECS / 1_000_000.0, dtx * 8.0 / POLL_INTERVAL_SECS / 1_000_000.0)
    } else {
      (0.0, 0.0)
    };
    out.net_interfaces.push(NetInterface {
      name: name.clone(),
      ip: if first_iface { ip.clone() } else { String::new() },
      rx_rate_mbps: rx_rate,
      tx_rate_mbps: tx_rate,
    });
    new_samples.net_prev.push((name.clone(), rx, tx));
    first_iface = false;
  }

  let disks = Disks::new_with_refreshed_list();
  for disk in disks.list() {
    if out.disk_mounts.len() >= 4 {
      break;
    }
    let total = disk.total_space();
    let avail = disk.available_space();
    let used = total - avail;
    let mp = disk.mount_point();
    let mount_str = mp.to_string_lossy().to_string();
    if out.disk_mounts.iter().any(|d| d.mount_point == mount_str) {
      continue;
    }
    out.disk_mounts.push(DiskMount {
      mount_point: mount_str,
      total_gb: total as f32 / BYTES_PER_GIB,
      used_gb: used as f32 / BYTES_PER_GIB,
      percent: if total > 0 { 100.0 * used as f32 / total as f32 } else { 0.0 },
    });
  }

  out.hostname = System::host_name().unwrap_or_default();
  out.kernel_version = System::kernel_version().unwrap_or_default();
  sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
  out.process_count = sys.processes().len() as u32;
  out.uptime_seconds = System::uptime();
  out.uptime_str = format_uptime(out.uptime_seconds);

  let la = System::load_average();
  out.load_avg_1 = la.one as f32;
  out.load_avg_5 = la.five as f32;
  out.load_avg_15 = la.fifteen as f32;

  *info = out;
  new_samples
}
