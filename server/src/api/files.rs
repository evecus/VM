use axum::{
    body::Body,
    extract::{Multipart, Query},
    http::{header, StatusCode},
    response::{Json, Response},
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
use tokio::fs as afs;
use tokio_util::io::ReaderStream;

#[derive(Deserialize)]
pub struct PathQuery {
    pub path: Option<String>,
}

#[derive(Deserialize)]
pub struct PathBody {
    pub path: String,
}

#[derive(Deserialize)]
pub struct RenameBody {
    #[serde(rename = "oldPath")]
    pub old_path: String,
    #[serde(rename = "newPath")]
    pub new_path: String,
}

#[derive(Deserialize)]
pub struct WriteBody {
    pub path: String,
    pub content: String,
}

fn mode_string(mode: u32) -> String {
    let chars = ['r', 'w', 'x'];
    let mut s = String::with_capacity(10);
    // type
    s.push(if mode & 0o170000 == 0o040000 { 'd' } else if mode & 0o170000 == 0o120000 { 'l' } else { '-' });
    for shift in [6u32, 3, 0] {
        for (i, &c) in chars.iter().enumerate() {
            s.push(if mode & (1 << (shift + 2 - i as u32)) != 0 { c } else { '-' });
        }
    }
    s
}

pub async fn list_files(
    Query(q): Query<PathQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let path = q.path.as_deref().unwrap_or("/");

    let entries = fs::read_dir(path).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))
    })?;

    let mut items: Vec<Value> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let full_path = format!("{}/{}", path.trim_end_matches('/'), name);

        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        let is_symlink = metadata.file_type().is_symlink();
        let is_dir = if is_symlink {
            fs::metadata(&full_path).map(|m| m.is_dir()).unwrap_or(false)
        } else {
            metadata.is_dir()
        };

        let symlink_dest = if is_symlink {
            fs::read_link(&full_path).ok().map(|p| p.to_string_lossy().to_string())
        } else {
            None
        };

        let mod_time = metadata.modified().ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let mut item = json!({
            "name": name,
            "path": full_path,
            "isDir": is_dir,
            "size": metadata.len(),
            "mode": mode_string(metadata.permissions().mode()),
            "modTime": mod_time,
            "isSymlink": is_symlink,
            "owner": metadata.uid(),
            "group": metadata.gid(),
        });

        if let Some(dest) = symlink_dest {
            item["symlinkDest"] = json!(dest);
        }

        items.push(item);
    }

    // Sort: dirs first, then alphabetically
    items.sort_by(|a, b| {
        let a_dir = a["isDir"].as_bool().unwrap_or(false);
        let b_dir = b["isDir"].as_bool().unwrap_or(false);
        b_dir.cmp(&a_dir).then_with(|| {
            a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or(""))
        })
    });

    Ok(Json(json!({"path": path, "items": items})))
}

pub async fn download_file(
    Query(q): Query<PathQuery>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let path = q.path.ok_or_else(|| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": "path required"})))
    })?;

    let meta = fs::metadata(&path).map_err(|_| {
        (StatusCode::NOT_FOUND, Json(json!({"error": "file not found"})))
    })?;
    if meta.is_dir() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "cannot download directory"}))));
    }

    let filename = PathBuf::from(&path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());

    let file = afs::File::open(&path).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))
    })?;

    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    Ok(Response::builder()
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}\"", filename))
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(body)
        .unwrap())
}

pub async fn upload_file(
    mut multipart: Multipart,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let mut dest_dir = "/".to_string();
    let mut file_data: Option<(String, Vec<u8>)> = None;

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": e.to_string()})))
    })? {
        let name = field.name().unwrap_or("").to_string();
        if name == "path" {
            dest_dir = field.text().await.unwrap_or_else(|_| "/".to_string());
        } else if name == "file" {
            let filename = field.file_name().unwrap_or("upload").to_string();
            let data = field.bytes().await.map_err(|e| {
                (StatusCode::BAD_REQUEST, Json(json!({"error": e.to_string()})))
            })?;
            file_data = Some((filename, data.to_vec()));
        }
    }

    let (filename, data) = file_data.ok_or_else(|| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": "no file provided"})))
    })?;

    let dest_path = format!("{}/{}", dest_dir.trim_end_matches('/'), filename);
    afs::write(&dest_path, &data).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))
    })?;

    Ok(Json(json!({"success": true, "path": dest_path})))
}

pub async fn mkdir(
    Json(req): Json<PathBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    fs::create_dir_all(&req.path).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))
    })?;
    Ok(Json(json!({"success": true})))
}

pub async fn rename_file(
    Json(req): Json<RenameBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    fs::rename(&req.old_path, &req.new_path).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))
    })?;
    Ok(Json(json!({"success": true})))
}

pub async fn delete_file(
    Json(req): Json<PathBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let meta = fs::metadata(&req.path).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))
    })?;
    if meta.is_dir() {
        fs::remove_dir_all(&req.path)
    } else {
        fs::remove_file(&req.path)
    }.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))
    })?;
    Ok(Json(json!({"success": true})))
}

pub async fn read_file(
    Query(q): Query<PathQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let path = q.path.ok_or_else(|| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": "path required"})))
    })?;

    let meta = fs::metadata(&path).map_err(|_| {
        (StatusCode::NOT_FOUND, Json(json!({"error": "file not found"})))
    })?;

    if meta.len() > 10 * 1024 * 1024 {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "file too large for editor (max 10MB)"}))));
    }

    let content = fs::read_to_string(&path).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))
    })?;

    Ok(Json(json!({
        "path": path,
        "content": content,
        "size": meta.len(),
    })))
}

pub async fn write_file(
    Json(req): Json<WriteBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // Create parent dirs if needed
    if let Some(parent) = Path::new(&req.path).parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&req.path, req.content.as_bytes()).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))
    })?;
    Ok(Json(json!({"success": true})))
}

pub async fn touch_file(
    Json(req): Json<PathBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&req.path)
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))
        })?;
    Ok(Json(json!({"success": true})))
}
