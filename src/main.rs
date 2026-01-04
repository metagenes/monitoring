use axum::{
    extract::Path,
    response::Html,
    routing::get,
    Json, Router,
};
use serde::Serialize;
use sysinfo::{System, Disks, Components};
use bollard::Docker;
use bollard::container::{ListContainersOptions, LogsOptions};
use futures_util::StreamExt;

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

async fn get_full_status() -> Json<FullStatus> {
    let mut sys = System::new_all();
    sys.refresh_all();
    
    // 1. Sensors
    let components = Components::new_with_refreshed_list();
    let sensors = components.iter()
        .map(|c| (c.label().to_string(), c.temperature()))
        .collect();

    // 2. Disk Usage
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

    // 3. Docker Containers
    let mut containers = Vec::new();
    if let Ok(docker) = Docker::connect_with_unix_defaults() {
        let options = Some(ListContainersOptions::<String> {
            all: true,
            ..Default::default()
        });

        if let Ok(list) = docker.list_containers(options).await {
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

async fn get_container_logs(Path(name): Path<String>) -> String {
    if let Ok(docker) = Docker::connect_with_unix_defaults() {
        let options = Some(LogsOptions::<String> {
            stdout: true,
            stderr: true,
            tail: "50".to_string(),
            ..Default::default()
        });

        let mut logs = docker.logs(&name, options);
        let mut output = String::new();

        while let Some(Ok(log)) = logs.next().await {
            output.push_str(&log.to_string());
        }

        if output.is_empty() { "Log tidak tersedia.".to_string() } else { output }
    } else {
        "Gagal akses Docker Socket.".to_string()
    }
}

async fn ui_handler() -> Html<&'static str> {
    Html(include_str!("index.html"))
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(ui_handler))
        .route("/api/status", get(get_full_status))
        .route("/api/logs/:name", get(get_container_logs));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:9999").await.unwrap();
    println!("🚀 Dashboard Prod siap di http://localhost:9999");
    axum::serve(listener, app).await.unwrap();
}