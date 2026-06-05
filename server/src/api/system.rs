use axum::{extract::Path, http::StatusCode, response::Json};
use serde_json::{json, Value};
use std::process::Command;
use sysinfo::{Disks, Networks, System};

fn run_cmd(prog: &str, args: &[&str]) -> String {
    Command::new(prog)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn get_public_ip() -> String {
    let out = run_cmd("ip", &["route", "get", "1.1.1.1"]);
    let fields: Vec<&str> = out.split_whitespace().collect();
    for (i, f) in fields.iter().enumerate() {
        if *f == "src" {
            if let Some(ip) = fields.get(i + 1) {
                return ip.to_string();
            }
        }
    }
    String::new()
}

fn get_ufw_status() -> Value {
    if Command::new("which").arg("ufw").output()
        .map(|o| !o.stdout.is_empty()).unwrap_or(false)
        || Command::new("ufw").arg("version").output().is_ok()
    {
        let out = run_cmd("ufw", &["status", "numbered"]);
        if out.is_empty() {
            return json!({"installed": true, "enabled": false, "ruleCount": 0});
        }
        let enabled = out.contains("Status: active");
        let rule_count = out.lines()
            .filter(|l| l.trim_start().starts_with('['))
            .count();
        json!({"installed": true, "enabled": enabled, "ruleCount": rule_count})
    } else {
        json!({"installed": false, "enabled": false, "ruleCount": 0})
    }
}

pub async fn get_system_info() -> Json<Value> {
    let mut sys = System::new_all();
    sys.refresh_all();

    // CPU
    let cpu_usage = sys.global_cpu_usage() as f64;
    let cpu_model = sys.cpus().first()
        .map(|c| c.brand().to_string())
        .unwrap_or_default();
    let cpu_cores = sys.cpus().len();

    // Memory
    let mem_total = sys.total_memory();
    let mem_used = sys.used_memory();
    let mem_free = sys.available_memory();
    let mem_percent = if mem_total > 0 {
        mem_used as f64 / mem_total as f64 * 100.0
    } else {
        0.0
    };

    // Disk (root)
    let disks = Disks::new_with_refreshed_list();
    let (disk_total, disk_used, disk_free, disk_percent) = disks
        .iter()
        .find(|d| d.mount_point().to_str() == Some("/"))
        .map(|d| {
            let total = d.total_space();
            let free = d.available_space();
            let used = total.saturating_sub(free);
            let pct = if total > 0 { used as f64 / total as f64 * 100.0 } else { 0.0 };
            (total, used, free, pct)
        })
        .unwrap_or((0, 0, 0, 0.0));

    // Host info (now associated functions in sysinfo 0.30+)
    let hostname = System::host_name().unwrap_or_default();
    let os_name = System::name().unwrap_or_default();
    let os_version = System::os_version().unwrap_or_default();
    let kernel = System::kernel_version().unwrap_or_default();
    let uptime = System::uptime();

    // Load average
    let load = System::load_average();

    // Network I/O (sum all interfaces)
    let networks = Networks::new_with_refreshed_list();
    let (net_sent, net_recv): (u64, u64) = networks.iter().fold((0, 0), |acc, (_, n)| {
        (acc.0 + n.total_transmitted(), acc.1 + n.total_received())
    });

    Json(json!({
        "cpu": {
            "percent": [cpu_usage],
            "model": cpu_model,
            "cores": cpu_cores,
        },
        "memory": {
            "total": mem_total,
            "used": mem_used,
            "free": mem_free,
            "percent": mem_percent,
        },
        "disk": {
            "total": disk_total,
            "used": disk_used,
            "free": disk_free,
            "percent": disk_percent,
        },
        "host": {
            "hostname": hostname,
            "os": os_name,
            "platform": os_name,
            "platformVersion": os_version,
            "kernelVersion": kernel,
            "uptime": uptime,
        },
        "load": {
            "load1": load.one,
            "load5": load.five,
            "load15": load.fifteen,
        },
        "network": {
            "bytesSent": net_sent,
            "bytesRecv": net_recv,
        },
        "publicIp": get_public_ip(),
        "ufw": get_ufw_status(),
    }))
}

pub async fn get_processes() -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let mut sys = System::new_all();
    sys.refresh_all();

    let processes: Vec<Value> = sys.processes().iter().map(|(pid, proc)| {
        let status = format!("{:?}", proc.status());
        json!({
            "pid": pid.as_u32(),
            "name": proc.name(),
            "cpu": proc.cpu_usage(),
            "memory": proc.memory() as f64 / sys.total_memory() as f64 * 100.0,
            "status": status,
            "user": proc.user_id().map(|u| u.to_string()).unwrap_or_default(),
            "cmdline": proc.cmd().join(" "),
        })
    }).collect();

    Ok(Json(json!({"processes": processes})))
}

pub async fn kill_process(
    Path(pid): Path<u32>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let result = Command::new("kill").arg(pid.to_string()).status();
    match result {
        Ok(s) if s.success() => Ok(Json(json!({"success": true}))),
        Ok(_) => {
            let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
            Ok(Json(json!({"success": true})))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )),
    }
}
