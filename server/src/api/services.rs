use axum::{extract::Path, http::StatusCode, response::Json};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::HashMap, fs, process::Command};

fn run_cmd(prog: &str, args: &[&str]) -> String {
    Command::new(prog)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn run_cmd_stderr(prog: &str, args: &[&str]) -> Result<(), String> {
    let output = Command::new(prog).args(args).output().map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        let msg = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if msg.is_empty() { format!("command failed: {} {:?}", prog, args) } else { msg })
    }
}

fn is_valid_service_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// 检测当前 init 系统：systemd / openrc / unknown
fn detect_init() -> &'static str {
    // systemd: /run/systemd/private 目录存在，或 systemctl 可用
    if std::path::Path::new("/run/systemd/private").exists() {
        return "systemd";
    }
    if Command::new("systemctl").arg("--version").output()
        .map(|o| o.status.success()).unwrap_or(false)
    {
        return "systemd";
    }
    // openrc: /sbin/openrc 或 /sbin/rc-status 存在
    if std::path::Path::new("/sbin/openrc").exists()
        || std::path::Path::new("/sbin/rc-status").exists()
        || std::path::Path::new("/usr/sbin/rc-status").exists()
    {
        return "openrc";
    }
    // openrc fallback: rc-status 可调用
    if Command::new("rc-status").arg("--version").output()
        .map(|o| o.status.success()).unwrap_or(false)
    {
        return "openrc";
    }
    "unknown"
}

// ── systemd ──────────────────────────────────────────────────────

fn get_services_systemd() -> Vec<Value> {
    let units_out = run_cmd("systemctl", &["list-units", "--type=service", "--all",
        "--no-pager", "--no-legend", "--plain"]);
    let files_out = run_cmd("systemctl", &["list-unit-files", "--type=service",
        "--no-pager", "--no-legend", "--plain"]);

    let enabled_map: HashMap<String, bool> = files_out.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() >= 2 { Some((f[0].to_string(), f[1] == "enabled")) } else { None }
        })
        .collect();

    units_out.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 4 { return None; }
            let name = f[0];
            let description = if f.len() > 4 { f[4..].join(" ") } else { String::new() };
            Some(json!({
                "name": name,
                "description": description,
                "loadState": f[1],
                "activeState": f[2],
                "subState": f[3],
                "enabled": enabled_map.get(name).copied().unwrap_or(false),
            }))
        })
        .collect()
}

// ── openrc ───────────────────────────────────────────────────────

fn get_services_openrc() -> Vec<Value> {
    // rc-status --all --nocolor 列出所有服务及状态
    let status_out = run_cmd("rc-status", &["--all", "--nocolor"]);
    // rc-update show 列出各 runlevel 里启用的服务
    let update_out = run_cmd("rc-update", &["show"]);

    let enabled_set: std::collections::HashSet<String> = update_out.lines()
        .filter_map(|line| {
            // 格式: " svcname | runlevel runlevel2"
            let parts: Vec<&str> = line.splitn(2, '|').collect();
            if parts.len() == 2 {
                let svc = parts[0].trim().to_string();
                if !svc.is_empty() { Some(svc) } else { None }
            } else { None }
        })
        .collect();

    let mut services: Vec<Value> = Vec::new();
    let mut current_runlevel = String::new();

    for line in status_out.lines() {
        // runlevel 行: "Runlevel: default" 或 "Runlevel: boot"
        if line.starts_with("Runlevel:") {
            current_runlevel = line.trim_start_matches("Runlevel:").trim().to_string();
            continue;
        }
        // 服务行:  " svcname    [ started ]" 或  "  svcname   [ stopped ]"
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }

        // 解析 "name  [ status ]"
        if let Some(bracket) = trimmed.find('[') {
            let name = trimmed[..bracket].trim().to_string();
            let status_raw = trimmed[bracket..].trim_matches(|c| c == '[' || c == ']').trim().to_string();
            if name.is_empty() { continue; }

            // 避免同一服务在多个 runlevel 重复出现
            if services.iter().any(|s: &Value| s["name"] == name) { continue; }

            let (active_state, sub_state) = match status_raw.as_str() {
                "started"  => ("active",   "running"),
                "stopped"  => ("inactive", "dead"),
                "crashed"  => ("failed",   "crashed"),
                "starting" => ("activating", "start"),
                "stopping" => ("deactivating", "stop"),
                _          => ("inactive", status_raw.as_str()),
            };

            services.push(json!({
                "name": name,
                "description": "",
                "loadState": "loaded",
                "activeState": active_state,
                "subState": sub_state,
                "enabled": enabled_set.contains(&name),
                "runlevel": current_runlevel,
            }));
        }
    }

    // 如果 rc-status 没输出（老版本 OpenRC），fallback 到 ls /etc/init.d
    if services.is_empty() {
        if let Ok(entries) = fs::read_dir("/etc/init.d") {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') { continue; }
                let active_state = {
                    let out = run_cmd("rc-service", &[&name, "status"]);
                    if out.contains("started") { "active" } else { "inactive" }
                };
                services.push(json!({
                    "name": name,
                    "description": "",
                    "loadState": "loaded",
                    "activeState": active_state,
                    "subState": if active_state == "active" { "running" } else { "dead" },
                    "enabled": enabled_set.contains(&name),
                    "runlevel": "default",
                }));
            }
        }
    }

    services
}

// ── handlers ─────────────────────────────────────────────────────

pub async fn get_services() -> Json<Value> {
    let init = detect_init();
    let services = match init {
        "systemd" => get_services_systemd(),
        "openrc"  => get_services_openrc(),
        _         => vec![],
    };
    Json(json!({ "initSystem": init, "services": services }))
}

#[derive(Deserialize)]
pub struct ActionBody {
    pub action: String,
}

pub async fn service_action(
    Path(name): Path<String>,
    Json(req): Json<ActionBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let allowed = ["start", "stop", "restart", "reload", "enable", "disable"];
    if !allowed.contains(&req.action.as_str()) {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "invalid action"}))));
    }
    if !is_valid_service_name(&name) {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "invalid service name"}))));
    }

    match detect_init() {
        "systemd" => {
            run_cmd_stderr("systemctl", &[&req.action, &name])
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))))?;
        }
        "openrc" => {
            match req.action.as_str() {
                "start" | "stop" | "restart" | "reload" => {
                    run_cmd_stderr("rc-service", &[&name, &req.action])
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))))?;
                }
                "enable" => {
                    run_cmd_stderr("rc-update", &["add", &name, "default"])
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))))?;
                }
                "disable" => {
                    run_cmd_stderr("rc-update", &["del", &name])
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))))?;
                }
                _ => {}
            }
        }
        _ => {
            return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "unsupported init system"}))));
        }
    }
    Ok(Json(json!({"success": true})))
}

pub async fn get_service_unit(
    Path(name): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match detect_init() {
        "systemd" => {
            let name = name.trim_end_matches(".service");
            let paths = [
                format!("/etc/systemd/system/{}.service", name),
                format!("/lib/systemd/system/{}.service", name),
                format!("/usr/lib/systemd/system/{}.service", name),
            ];
            for path in &paths {
                if let Ok(content) = fs::read_to_string(path) {
                    return Ok(Json(json!({"content": content, "path": path, "initSystem": "systemd"})));
                }
            }
            Err((StatusCode::NOT_FOUND, Json(json!({"error": "unit file not found"}))))
        }
        "openrc" => {
            let path = format!("/etc/init.d/{}", name);
            match fs::read_to_string(&path) {
                Ok(content) => Ok(Json(json!({"content": content, "path": path, "initSystem": "openrc"}))),
                Err(_) => Err((StatusCode::NOT_FOUND, Json(json!({"error": "init script not found"})))),
            }
        }
        _ => Err((StatusCode::BAD_REQUEST, Json(json!({"error": "unsupported init system"})))),
    }
}

// ── create / update / delete ──────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateServiceBody {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "execStart")]
    pub exec_start: String,
    #[serde(rename = "workingDir")]
    pub working_dir: Option<String>,
    pub user: Option<String>,
    // systemd fields
    pub restart: Option<String>,
    #[serde(rename = "wantedBy")]
    pub wanted_by: Option<String>,
    // openrc fields
    pub runlevel: Option<String>,
    pub respawn: Option<bool>,
}

fn build_systemd_unit(
    desc: Option<&str>, exec_start: &str, working_dir: Option<&str>,
    user: Option<&str>, restart: &str, wanted_by: &str,
) -> String {
    let mut s = String::from("[Unit]\n");
    if let Some(d) = desc.filter(|d| !d.is_empty()) {
        s.push_str(&format!("Description={}\n", d));
    }
    s.push_str("After=network.target\n\n[Service]\nType=simple\n");
    if let Some(u) = user.filter(|u| !u.is_empty()) {
        s.push_str(&format!("User={}\n", u));
    }
    if let Some(wd) = working_dir.filter(|w| !w.is_empty()) {
        s.push_str(&format!("WorkingDirectory={}\n", wd));
    }
    s.push_str(&format!("ExecStart={}\n", exec_start));
    s.push_str(&format!("Restart={}\nRestartSec=5\n\n[Install]\nWantedBy={}\n", restart, wanted_by));
    s
}

fn build_openrc_script(
    name: &str, desc: Option<&str>, exec_start: &str,
    working_dir: Option<&str>, user: Option<&str>, respawn: bool,
) -> String {
    let mut s = String::from("#!/sbin/openrc-run\n\n");
    if let Some(d) = desc.filter(|d| !d.is_empty()) {
        s.push_str(&format!("description=\"{}\"\n", d));
    }
    s.push_str(&format!("command=\"{}\"\n", exec_start));
    s.push_str(&format!("command_background=true\n"));
    s.push_str(&format!("pidfile=\"/run/{}.pid\"\n", name));
    if let Some(u) = user.filter(|u| !u.is_empty()) {
        s.push_str(&format!("command_user=\"{}\"\n", u));
    }
    if let Some(wd) = working_dir.filter(|w| !w.is_empty()) {
        s.push_str(&format!("directory=\"{}\"\n", wd));
    }
    if respawn {
        s.push_str("\nrespawn\nrespawn_delay=5\nrespawn_max=0\n");
    }
    s.push_str("\ndepend() {\n    need net\n}\n");
    s
}

pub async fn create_service(
    Json(req): Json<CreateServiceBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if req.name.is_empty() || req.exec_start.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "name and execStart required"}))));
    }
    if !is_valid_service_name(&req.name) {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "invalid service name"}))));
    }

    match detect_init() {
        "systemd" => {
            let restart = req.restart.as_deref().filter(|s| !s.is_empty()).unwrap_or("on-failure");
            let wanted_by = req.wanted_by.as_deref().filter(|s| !s.is_empty()).unwrap_or("multi-user.target");
            let unit = build_systemd_unit(
                req.description.as_deref(), &req.exec_start,
                req.working_dir.as_deref(), req.user.as_deref(), restart, wanted_by,
            );
            let path = format!("/etc/systemd/system/{}.service", req.name);
            fs::write(&path, unit).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
            run_cmd("systemctl", &["daemon-reload"]);
            Ok(Json(json!({"success": true, "path": path})))
        }
        "openrc" => {
            let script = build_openrc_script(
                &req.name, req.description.as_deref(), &req.exec_start,
                req.working_dir.as_deref(), req.user.as_deref(),
                req.respawn.unwrap_or(true),
            );
            let path = format!("/etc/init.d/{}", req.name);
            fs::write(&path, &script).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
            // chmod +x
            run_cmd("chmod", &["+x", &path]);
            // 加入 runlevel
            let runlevel = req.runlevel.as_deref().filter(|s| !s.is_empty()).unwrap_or("default");
            run_cmd("rc-update", &["add", &req.name, runlevel]);
            Ok(Json(json!({"success": true, "path": path})))
        }
        _ => Err((StatusCode::BAD_REQUEST, Json(json!({"error": "unsupported init system"})))),
    }
}

#[derive(Deserialize)]
pub struct UpdateServiceBody {
    pub description: Option<String>,
    #[serde(rename = "execStart")]
    pub exec_start: String,
    #[serde(rename = "workingDir")]
    pub working_dir: Option<String>,
    pub user: Option<String>,
    pub restart: Option<String>,
    #[serde(rename = "wantedBy")]
    pub wanted_by: Option<String>,
    pub runlevel: Option<String>,
    pub respawn: Option<bool>,
}

pub async fn update_service(
    Path(name): Path<String>,
    Json(req): Json<UpdateServiceBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match detect_init() {
        "systemd" => {
            let name = name.trim_end_matches(".service");
            let restart = req.restart.as_deref().filter(|s| !s.is_empty()).unwrap_or("on-failure");
            let wanted_by = req.wanted_by.as_deref().filter(|s| !s.is_empty()).unwrap_or("multi-user.target");
            let unit = build_systemd_unit(
                req.description.as_deref(), &req.exec_start,
                req.working_dir.as_deref(), req.user.as_deref(), restart, wanted_by,
            );
            let path = format!("/etc/systemd/system/{}.service", name);
            fs::write(&path, unit).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
            run_cmd("systemctl", &["daemon-reload"]);
            Ok(Json(json!({"success": true})))
        }
        "openrc" => {
            let script = build_openrc_script(
                &name, req.description.as_deref(), &req.exec_start,
                req.working_dir.as_deref(), req.user.as_deref(),
                req.respawn.unwrap_or(true),
            );
            let path = format!("/etc/init.d/{}", name);
            fs::write(&path, &script).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
            run_cmd("chmod", &["+x", &path]);
            Ok(Json(json!({"success": true})))
        }
        _ => Err((StatusCode::BAD_REQUEST, Json(json!({"error": "unsupported init system"})))),
    }
}

pub async fn delete_service(
    Path(name): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if !is_valid_service_name(&name) {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "invalid service name"}))));
    }
    match detect_init() {
        "systemd" => {
            let svc = format!("{}.service", name.trim_end_matches(".service"));
            run_cmd("systemctl", &["stop", &svc]);
            run_cmd("systemctl", &["disable", &svc]);
            let path = format!("/etc/systemd/system/{}", svc);
            fs::remove_file(&path).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
            run_cmd("systemctl", &["daemon-reload"]);
        }
        "openrc" => {
            run_cmd("rc-service", &[&name, "stop"]);
            run_cmd("rc-update", &["del", &name]);
            let path = format!("/etc/init.d/{}", name);
            fs::remove_file(&path).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
        }
        _ => {
            return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "unsupported init system"}))));
        }
    }
    Ok(Json(json!({"success": true})))
}
