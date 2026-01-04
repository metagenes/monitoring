use axum::{
    extract::{Path, State},
    response::Html,
    routing::get,
    Json, Router,
};
use serde::Serialize;
use sysinfo::{System, Disks, Components};
use bollard::Docker;
use bollard::container::{ListContainersOptions, LogsOptions};
use futures_util::StreamExt;
use std::sync::{Arc, Mutex};
use chrono::{Duration, Utc};
use jemallocator::Jemalloc;

// Menggunakan Jemalloc untuk manajemen memori yang agresif (menghindari RAM membengkak)
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

// Shared State untuk menyimpan objek System dan Docker client agar persisten
struct AppState {
    sys: Mutex<System>,
    docker: Docker,
}

#[derive(Serialize)]
struct DiskInfo {
    name: String,
    mount_point: String,
    total_gb: u64,
    used_gb: u64,
}

#[derive(Serialize)]
struct ContainerInfo {
    name: String,
    status: String,
    state: String,
    ports: String,
}

#[derive(Serialize)]
struct FullStatus {
    cpu_usage: f32,
    ram_used_mb: u64,
    ram_total_mb: u64,
    sensors: Vec<(String, f32)>,
    disks: Vec<DiskInfo>,
    containers: Vec<ContainerInfo>,
}

async fn get_full_status(State(state): State<Arc<AppState>>) -> Json<FullStatus> {
    // 1. Ambil data sistem di dalam scope terpisah agar MutexGuard segera di-drop sebelum .await
    let (cpu_usage, ram_used, ram_total, sensors, disks) = {
        let mut sys = state.sys.lock().unwrap();
        sys.refresh_cpu();
        sys.refresh_memory();

        let cpu = sys.global_cpu_info().cpu_usage();
        let r_used = sys.used_memory() / 1024 / 1024;
        let r_total = sys.total_memory() / 1024 / 1024;

        let components = Components::new_with_refreshed_list();
        let sens = components.iter()
            .map(|c| (c.label().to_string(), c.temperature()))
            .collect::<Vec<_>>();

        let disks_list = Disks::new_with_refreshed_list();
        let dsk = disks_list.iter().map(|d| {
            let total = d.total_space();
            let available = d.available_space();
            DiskInfo {
                name: d.name().to_string_lossy().into_owned(),
                mount_point: d.mount_point().to_string_lossy().into_owned(),
                total_gb: total / 1024 / 1024 / 1024,
                used_gb: (total - available) / 1024 / 1024 / 1024,
            }
        }).collect::<Vec<_>>();

        (cpu, r_used, r_total, sens, dsk)
    };

    // 2. Ambil data Docker menggunakan shared client
    let mut containers = Vec::new();
    let options = Some(ListContainersOptions::<String> {
        all: true,
        ..Default::default()
    });

    if let Ok(list) = state.docker.list_containers(options).await {
        for c in list {
            let port_info = c.ports.unwrap_or_default()
                .iter()
                .filter_map(|p| {
                    if let Some(pub_p) = p.public_port {
                        let priv_p = p.private_port;
                        let proto = p.typ.as_ref()
                            .map(|t| format!("{:?}", t).to_lowercase())
                            .unwrap_or_else(|| "tcp".to_string());
                        Some(format!("{}:{} ({})", pub_p, priv_p, proto))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");

            containers.push(ContainerInfo {
                name: c.names.unwrap_or_default().join(", ").replace("/", ""),
                status: c.status.unwrap_or_default(),
                state: c.state.unwrap_or_default(),
                ports: if port_info.is_empty() { "-".to_string() } else { port_info },
            });
        }
    }

    Json(FullStatus {
        cpu_usage,
        ram_used_mb: ram_used,
        ram_total_mb: ram_total,
        sensors,
        disks,
        containers,
    })
}

async fn get_container_logs(Path(name): Path<String>, State(state): State<Arc<AppState>>) -> String {
    // Hitung timestamp 30 menit yang lalu menggunakan Chrono
    let thirty_minutes_ago = Utc::now() - Duration::minutes(30);
    let since_timestamp = thirty_minutes_ago.timestamp();

    let options = Some(LogsOptions::<String> {
        stdout: true,
        stderr: true,
        since: since_timestamp, // Hanya ambil log sejak 30 menit lalu
        tail: "100".to_string(), // Tetap beri batas baris sebagai safety guard RAM
        ..Default::default()
    });

    let mut logs = state.docker.logs(&name, options);
    let mut output = String::new();

    while let Some(Ok(log)) = logs.next().await {
        output.push_str(&log.to_string());
    }

    if output.is_empty() { 
        format!("Tidak ada aktivitas log untuk container '{}' dalam 30 menit terakhir.", name)
    } else { 
        output 
    }
}

async fn ui_handler() -> Html<&'static str> {
    Html(include_str!("index.html"))
}

#[tokio::main]
async fn main() {
    // Inisialisasi Shared State
    let shared_state = Arc::new(AppState {
        sys: Mutex::new(System::new_all()),
        docker: Docker::connect_with_unix_defaults().expect("Gagal akses Docker Socket"),
    });

    let app = Router::new()
        .route("/", get(ui_handler))
        .route("/api/status", get(get_full_status))
        .route("/api/logs/:name", get(get_container_logs))
        .with_state(shared_state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:9999").await.unwrap();
    println!("🚀 Dashboard Teroptimasi (<10MB RAM) running di http://localhost:9999");
    axum::serve(listener, app).await.unwrap();
}