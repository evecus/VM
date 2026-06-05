mod api;
mod config;
mod ws;

use anyhow::Result;
use axum::{
    extract::{Query, Request, State},
    http::{HeaderMap, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Json, Response},
    routing::{delete, get, post, put},
    Router,
};
use clap::Parser;
use serde::Deserialize;
use serde_json::json;
use std::{net::SocketAddr, sync::Arc};
use tower_http::cors::{Any, CorsLayer};
use tracing::{info, Level};
use tracing_subscriber::EnvFilter;

/// VM Server version injected at build time
pub static VERSION: &str = match option_env!("VM_VERSION") {
    Some(v) => v,
    None => "dev",
};

#[derive(Parser)]
#[command(name = "vm-server", about = "VM Server Agent")]
struct Args {
    /// Config file path
    #[arg(short = 'c', default_value = "config.yaml")]
    config: String,
}

#[derive(Clone)]
struct AppState {
    token: Arc<String>,
}

#[derive(Deserialize)]
struct TokenQuery {
    token: Option<String>,
}

async fn auth_middleware(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<TokenQuery>,
    request: Request,
    next: Next,
) -> Response {
    let token = headers
        .get("X-Token")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or(q.token);

    match token {
        Some(t) if t == *state.token => next.run(request).await,
        _ => (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "unauthorized"})),
        )
            .into_response(),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Init tracing — only errors by default
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();

    let args = Args::parse();
    let cfg = config::Config::load(&args.config)?;

    let state = AppState {
        token: Arc::new(cfg.token.clone()),
    };

    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS])
        .allow_headers(Any)
        .allow_origin(Any)
        .expose_headers(Any);

    // Auth middleware layer for protected routes
    let auth = middleware::from_fn_with_state(state.clone(), auth_middleware);

    // Protected API routes
    let protected = Router::new()
        // System
        .route("/system/info", get(api::system::get_system_info))
        .route("/system/processes", get(api::system::get_processes))
        .route("/system/processes/:pid", delete(api::system::kill_process))
        // Services
        .route("/services", get(api::services::get_services))
        .route("/services", post(api::services::create_service))
        .route("/services/:name/action", post(api::services::service_action))
        .route("/services/:name/unit", get(api::services::get_service_unit))
        .route("/services/:name", put(api::services::update_service))
        .route("/services/:name", delete(api::services::delete_service))
        // Files
        .route("/files", get(api::files::list_files))
        .route("/files/download", get(api::files::download_file))
        .route("/files/upload", post(api::files::upload_file))
        .route("/files/mkdir", post(api::files::mkdir))
        .route("/files/rename", post(api::files::rename_file))
        .route("/files", delete(api::files::delete_file))
        .route("/files/read", get(api::files::read_file))
        .route("/files/write", post(api::files::write_file))
        .route("/files/touch", post(api::files::touch_file))
        .layer(auth);

    // Token state for WebSocket auth
    let ws_state = state.clone();

    let app = Router::new()
        // Public routes
        .route("/api/version", get(move || async { Json(json!({"version": VERSION})) }))
        .route("/api/auth/verify", post({
            let tok = state.token.clone();
            move |Json(req): Json<serde_json::Value>| {
                let tok = tok.clone();
                async move {
                    let provided = req.get("token").and_then(|v| v.as_str()).unwrap_or("");
                    if provided == tok.as_str() {
                        (StatusCode::OK, Json(json!({"success": true})))
                    } else {
                        (StatusCode::UNAUTHORIZED, Json(json!({"error": "invalid token"})))
                    }
                }
            }
        }))
        // WebSocket terminal (auth via query param)
        .route("/ws/terminal", get({
            move |headers: HeaderMap, Query(q): Query<TokenQuery>, ws: axum::extract::ws::WebSocketUpgrade| {
                let tok = ws_state.token.clone();
                async move {
                    let token = headers
                        .get("X-Token")
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_string())
                        .or(q.token);
                    match token {
                        Some(t) if t == *tok => ws.on_upgrade(ws::terminal::handle_ws).into_response(),
                        _ => (StatusCode::UNAUTHORIZED, Json(json!({"error": "unauthorized"}))).into_response(),
                    }
                }
            }
        }))
        // Protected API
        .nest("/api", protected)
        .layer(cors);

    let addr: SocketAddr = format!("0.0.0.0:{}", cfg.port).parse()?;
    info!("VM Server {} starting on {} (TLS: {})", VERSION, addr, cfg.tls.enabled);

    if cfg.tls.enabled {
        use axum_server::tls_rustls::RustlsConfig;
        let tls_config = RustlsConfig::from_pem_file(&cfg.tls.cert, &cfg.tls.key).await?;
        axum_server::bind_rustls(addr, tls_config)
            .serve(app.into_make_service())
            .await?;
    } else {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        println!("VM Server {} listening on {} (TLS: false)", VERSION, addr);
        axum::serve(listener, app).await?;
    }

    Ok(())
}
