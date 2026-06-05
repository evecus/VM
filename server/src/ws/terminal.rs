use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Deserialize;
use std::{
    io::{Read, Write},
    sync::Arc,
};
use tokio::sync::Mutex;

#[derive(Deserialize)]
struct ResizeMsg {
    #[serde(rename = "type")]
    msg_type: String,
    rows: u16,
    cols: u16,
}

pub async fn handle_ws(socket: WebSocket) {
    if let Err(e) = run_terminal(socket).await {
        tracing::error!("terminal error: {}", e);
    }
}

async fn run_terminal(socket: WebSocket) -> anyhow::Result<()> {
    let pty_system = native_pty_system();

    let pair = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
    let mut cmd = CommandBuilder::new(&shell);
    cmd.env("TERM", "xterm-256color");
    cmd.env("LANG", "en_US.UTF-8");

    let _child = pair.slave.spawn_command(cmd)?;

    let mut pty_reader = pair.master.try_clone_reader()?;
    let pty_writer = Arc::new(Mutex::new(pair.master.take_writer()?));
    let pty_master = Arc::new(Mutex::new(pair.master));

    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Spawn blocking thread to read PTY output
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match pty_reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    // PTY -> WebSocket forwarding task
    let send_task = tokio::spawn(async move {
        while let Some(data) = rx.recv().await {
            if ws_sender.send(Message::Binary(data)).await.is_err() {
                break;
            }
        }
    });

    // WebSocket -> PTY
    while let Some(msg) = ws_receiver.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(_) => break,
        };

        let data: Vec<u8> = match msg {
            Message::Text(t) => t.into_bytes(),
            Message::Binary(b) => b,
            Message::Close(_) => break,
            _ => continue,
        };

        // Try parse as resize message
        if let Ok(resize) = serde_json::from_slice::<ResizeMsg>(&data) {
            if resize.msg_type == "resize" {
                let master = pty_master.lock().await;
                let _ = master.resize(PtySize {
                    rows: resize.rows,
                    cols: resize.cols,
                    pixel_width: 0,
                    pixel_height: 0,
                });
                continue;
            }
        }

        // Write raw input to PTY
        let mut writer = pty_writer.lock().await;
        if writer.write_all(&data).is_err() {
            break;
        }
    }

    send_task.abort();
    Ok(())
}
