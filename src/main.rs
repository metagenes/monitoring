use axum::{response::Html, routing::get, Json, Router};
use serde::Serialize;
use sysinfo::{System, Disks, Components};
use bollard::Docker;
use bollard::container::ListContainersOptions;

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

async fn get_full_status() -> Json<FullStatus> {
    let mut sys = System::new_all();
    sys.refresh_all();
    
    // 1. Sensors (Suhu)
    let components = Components::new_with_refreshed_list();
    let sensors = components.iter()
        .map(|c| (c.label().to_string(), c.temperature()))
        .collect();

    // 2. Disk Usage (df -h)
    let disks_list = Disks::new_with_refreshed_list();
    let disks = disks_list.iter().map(|d| {
        let total = d.total_space();
        let available = d.available_space();
        DiskInfo {
            name: d.name().to_string_lossy().into_owned(),
            mount_point: d.mount_point().to_string_lossy().into_owned(),
            total_gb: total / 1024 / 1024 / 1024,
            used_gb: (total - available) / 1024 / 1024 / 1024,
        }
    }).collect();

    // 3. Docker Stats
    let mut containers = Vec::new();
    if let Ok(docker) = Docker::connect_with_unix_defaults() {
        let options = Some(ListContainersOptions::<String> {
            all: true,
            ..Default::default()
        });
        if let Ok(list) = docker.list_containers(options).await {
            for c in list {
                containers.push(ContainerInfo {
                    name: c.names.unwrap_or_default().join(", "),
                    status: c.status.unwrap_or_default(),
                    state: c.state.unwrap_or_default(),
                });
            }
        }
    }

    Json(FullStatus {
        cpu_usage: sys.global_cpu_info().cpu_usage(),
        ram_used_mb: sys.used_memory() / 1024 / 1024,
        ram_total_mb: sys.total_memory() / 1024 / 1024,
        sensors,
        disks,
        containers,
    })
}

async fn ui_handler() -> Html<&'static str> {
    Html(include_str!("index.html"))
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(ui_handler))
        .route("/api/status", get(get_full_status));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:9999").await.unwrap();
    println!("🚀 Dashboard Dashboard di http://localhost:9999");
    axum::serve(listener, app).await.unwrap();
}