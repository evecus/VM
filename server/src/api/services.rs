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

pub async fn get_services() -> Json<Value> {
    let units_out = run_cmd("systemctl", &["list-units", "--type=service", "--all",
        "--no-pager", "--no-legend", "--plain"]);

    let files_out = run_cmd("systemctl", &["list-unit-files", "--type=service",
        "--no-pager", "--no-legend", "--plain"]);

    // Build enabled map
    let enabled_map: HashMap<String, bool> = files_out.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() >= 2 {
                Some((fields[0].to_string(), fields[1] == "enabled"))
            } else {
                None
            }
        })
        .collect();

    let services: Vec<Value> = units_out.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 4 {
                return None;
            }
            let name = fields[0];
            let load_state = fields[1];
            let active_state = fields[2];
            let sub_state = fields[3];
            let description = if fields.len() > 4 { fields[4..].join(" ") } else { String::new() };

            Some(json!({
                "name": name,
                "description": description,
                "loadState": load_state,
                "activeState": active_state,
                "subState": sub_state,
                "enabled": enabled_map.get(name).copied().unwrap_or(false),
            }))
        })
        .collect();

    Json(json!({"services": services}))
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

    run_cmd_stderr("systemctl", &[&req.action, &name]).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))
    })?;
    Ok(Json(json!({"success": true})))
}

pub async fn get_service_unit(
    Path(name): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let name = name.trim_end_matches(".service");
    let paths = [
        format!("/etc/systemd/system/{}.service", name),
        format!("/lib/systemd/system/{}.service", name),
        format!("/usr/lib/systemd/system/{}.service", name),
    ];
    for path in &paths {
        if let Ok(content) = fs::read_to_string(path) {
            return Ok(Json(json!({"content": content, "path": path})));
        }
    }
    Err((StatusCode::NOT_FOUND, Json(json!({"error": "unit file not found"}))))
}

#[derive(Deserialize)]
pub struct CreateServiceBody {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "execStart")]
    pub exec_start: String,
    #[serde(rename = "workingDir")]
    pub working_dir: Option<String>,
    pub user: Option<String>,
    pub restart: Option<String>,
    #[serde(rename = "wantedBy")]
    pub wanted_by: Option<String>,
}

fn build_unit(
    desc: Option<&str>,
    exec_start: &str,
    working_dir: Option<&str>,
    user: Option<&str>,
    restart: &str,
    wanted_by: &str,
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

pub async fn create_service(
    Json(req): Json<CreateServiceBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if req.name.is_empty() || req.exec_start.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "name and execStart required"}))));
    }
    if !is_valid_service_name(&req.name) {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "invalid service name"}))));
    }

    let restart = req.restart.as_deref().filter(|s| !s.is_empty()).unwrap_or("on-failure");
    let wanted_by = req.wanted_by.as_deref().filter(|s| !s.is_empty()).unwrap_or("multi-user.target");

    let unit = build_unit(
        req.description.as_deref(),
        &req.exec_start,
        req.working_dir.as_deref(),
        req.user.as_deref(),
        restart,
        wanted_by,
    );

    let path = format!("/etc/systemd/system/{}.service", req.name);
    fs::write(&path, unit).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))
    })?;
    run_cmd("systemctl", &["daemon-reload"]);
    Ok(Json(json!({"success": true, "path": path})))
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
}

pub async fn update_service(
    Path(name): Path<String>,
    Json(req): Json<UpdateServiceBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let name = name.trim_end_matches(".service");
    let restart = req.restart.as_deref().filter(|s| !s.is_empty()).unwrap_or("on-failure");
    let wanted_by = req.wanted_by.as_deref().filter(|s| !s.is_empty()).unwrap_or("multi-user.target");

    let unit = build_unit(
        req.description.as_deref(),
        &req.exec_start,
        req.working_dir.as_deref(),
        req.user.as_deref(),
        restart,
        wanted_by,
    );

    let path = format!("/etc/systemd/system/{}.service", name);
    fs::write(&path, unit).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))
    })?;
    run_cmd("systemctl", &["daemon-reload"]);
    Ok(Json(json!({"success": true})))
}

pub async fn delete_service(
    Path(name): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let name = name.trim_end_matches(".service");
    if !is_valid_service_name(name) {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "invalid service name"}))));
    }
    run_cmd("systemctl", &["stop", &format!("{}.service", name)]);
    run_cmd("systemctl", &["disable", &format!("{}.service", name)]);
    let path = format!("/etc/systemd/system/{}.service", name);
    fs::remove_file(&path).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))
    })?;
    run_cmd("systemctl", &["daemon-reload"]);
    Ok(Json(json!({"success": true})))
}
