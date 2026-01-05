use axum::{
    extract::{Path, State},
    response::Html,
    routing::get,
    Json, Router,
};
use serde::Serialize;
use sysinfo::{System, Disks, Components, Networks};
use bollard::Docker;
use bollard::container::{ListContainersOptions, LogsOptions};
use bollard::image::ListImagesOptions;
use futures_util::StreamExt;
use std::sync::{Arc, Mutex};
use std::collections::HashSet;
use chrono::{Duration, Utc};
use jemallocator::Jemalloc;

#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

struct AppState {
    sys: Mutex<System>,
    networks: Mutex<Networks>,
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
struct ImageInfo {
    repo: String,
    tag: String,
    id: String,
    size_gb: f64,
    in_use: bool,
}

#[derive(Serialize)]
struct NetworkInfo {
    name: String,
    rx_bytes: u64,
    tx_bytes: u64,
}

#[derive(Serialize)]
struct ProcessInfo {
    name: String,
    ram_mb: u64,
}

#[derive(Serialize)]
struct FullStatus {
    cpu_usage: f32,
    ram_used_mb: u64,
    ram_total_mb: u64,
    swap_used_mb: u64,
    swap_total_mb: u64,
    uptime_secs: u64,
    load_avg: [f64; 3],
    internet_latency_ms: f64,
    networks: Vec<NetworkInfo>,
    processes: Vec<ProcessInfo>,
    sensors: Vec<(String, f32)>,
    disks: Vec<DiskInfo>,
    containers: Vec<ContainerInfo>,
    images: Vec<ImageInfo>,
}

async fn measure_latency() -> f64 {
    let start = std::time::Instant::now();
    // Connect to Cloudflare DNS (1.1.1.1:53) - fast & reliable
    if let Ok(_) = tokio::net::TcpStream::connect("1.1.1.1:53").await {
        return start.elapsed().as_secs_f64() * 1000.0;
    }
    // Fallback to Google DNS (8.8.8.8:53)
    if let Ok(_) = tokio::net::TcpStream::connect("8.8.8.8:53").await {
        return start.elapsed().as_secs_f64() * 1000.0;
    }
    0.0
}

fn get_top_ram_processes() -> Vec<ProcessInfo> {
    let mut procs = Vec::new();
    let page_size = 4096; // Standard 4KB page size
    
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if !meta.is_dir() { continue; }
            }
            
            let name = entry.file_name();
            let pid_str = name.to_string_lossy();
            if !pid_str.chars().all(char::is_numeric) { continue; }

            let path = entry.path();
            // Read statm for memory usage (faster than status)
            // Format: size resident shared text lib data dt
            if let Ok(statm) = std::fs::read_to_string(path.join("statm")) {
                let parts: Vec<&str> = statm.split_whitespace().collect();
                if let Some(resident_str) = parts.get(1) {
                    if let Ok(resident_pages) = resident_str.parse::<u64>() {
                        let ram_mb = (resident_pages * page_size) / 1024 / 1024;
                        // Avoid reading files for tiny processes to save IO
                        if ram_mb > 10 { 
                            let mut name = "unknown".to_string();
                            if let Ok(comm) = std::fs::read_to_string(path.join("comm")) {
                                name = comm.trim().to_string();
                            }
                            procs.push(ProcessInfo { name, ram_mb });
                        }
                    }
                }
            }
        }
    }
    
    // Sort desc and take top 3
    procs.sort_by(|a, b| b.ram_mb.cmp(&a.ram_mb));
    procs.truncate(3);
    procs
}

async fn get_full_status(State(state): State<Arc<AppState>>) -> Json<FullStatus> {
    // 1. Hardware Stats
    let (cpu_usage, ram_used, ram_total, swap_used, swap_total, uptime, load_avg, sensors, disks) = {
        let mut sys = state.sys.lock().unwrap();
        
        // Refresh only what we need!
        sys.refresh_cpu();
        sys.refresh_memory();

        let cpu = sys.global_cpu_info().cpu_usage();
        let r_used = sys.used_memory() / 1024 / 1024;
        let r_total = sys.total_memory() / 1024 / 1024;
        let s_used = sys.used_swap() / 1024 / 1024;
        let s_total = sys.total_swap() / 1024 / 1024;
        let up = System::uptime();
        let load = System::load_average();

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

        (cpu, r_used, r_total, s_used, s_total, up, [load.one, load.five, load.fifteen], sens, dsk)
    };

    // 2. Networks
    let networks_list = {
        let mut nets = state.networks.lock().unwrap();
        nets.refresh();
        nets.iter().map(|(name, data)| NetworkInfo {
            name: name.clone(),
            rx_bytes: data.received(),
            tx_bytes: data.transmitted(),
        }).collect::<Vec<_>>()
    };

    // 3. New Checks (Latency & Processes) - Run async where possible or quick io
    let latency = measure_latency().await;
    // We run proc scan in a blocking task to avoiding stalling the async runtime significantly, 
    // although for /proc specific files it's usually fast. 
    // Given the constraints and simplicity, running inline is acceptable if fast, 
    // but spawn_blocking is safer for filesystem IO.
    let processes = tokio::task::spawn_blocking(get_top_ram_processes).await.unwrap_or_default();

    // 4. Docker Containers & Used Image Tracking
    let mut containers = Vec::new();
    let mut used_image_ids = HashSet::new();
    
    if let Ok(list) = state.docker.list_containers(Some(ListContainersOptions::<String> { all: true, ..Default::default() })).await {
        for c in list {
            if let Some(img_id) = &c.image_id {
                used_image_ids.insert(img_id.clone());
            }
            let port_info = c.ports.unwrap_or_default().iter().filter_map(|p| {
                p.public_port.map(|pub_p| format!("{}:{}", pub_p, p.private_port))
            }).collect::<Vec<_>>().join(", ");

            containers.push(ContainerInfo {
                name: c.names.unwrap_or_default().join("").replace("/", ""),
                status: c.status.unwrap_or_default(),
                state: c.state.unwrap_or_default(),
                ports: if port_info.is_empty() { "-".to_string() } else { port_info },
            });
        }
    }

    // 5. Docker Images
    let mut images = Vec::new();
    if let Ok(list) = state.docker.list_images(Some(ListImagesOptions::<String> { all: true, ..Default::default() })).await {
        for img in list {
            let repo_tag = img.repo_tags.first().cloned().unwrap_or_else(|| "none:none".to_string());
            let parts: Vec<&str> = repo_tag.split(':').collect();
            images.push(ImageInfo {
                repo: parts.get(0).unwrap_or(&"unknown").to_string(),
                tag: parts.get(1).unwrap_or(&"latest").to_string(),
                id: img.id.get(7..19).unwrap_or("unknown").to_string(),
                size_gb: img.size as f64 / 1024.0 / 1024.0 / 1024.0,
                in_use: used_image_ids.contains(&img.id),
            });
        }
    }

    Json(FullStatus { 
        cpu_usage, 
        ram_used_mb: ram_used, 
        ram_total_mb: ram_total, 
        swap_used_mb: swap_used,
        swap_total_mb: swap_total,
        uptime_secs: uptime,
        load_avg,
        internet_latency_ms: latency,
        networks: networks_list,
        processes,
        sensors, 
        disks, 
        containers, 
        images 
    })
}

async fn get_container_logs(Path(name): Path<String>, State(state): State<Arc<AppState>>) -> String {
    let since = (Utc::now() - Duration::minutes(30)).timestamp();
    let options = Some(LogsOptions::<String> {
        stdout: true, stderr: true, since, tail: "100".to_string(), ..Default::default()
    });

    let mut logs = state.docker.logs(&name, options);
    let mut output = String::new();
    while let Some(Ok(log)) = logs.next().await {
        output.push_str(&log.to_string());
    }
    if output.is_empty() { "No logs in last 30m.".to_string() } else { output }
}

async fn ui_handler() -> Html<&'static str> {
    Html(include_str!("index.html"))
}

#[tokio::main]
async fn main() {
    let networks = Networks::new_with_refreshed_list();
    let shared_state = Arc::new(AppState {
        sys: Mutex::new(System::new()),
        networks: Mutex::new(networks),
        docker: Docker::connect_with_unix_defaults().expect("Docker socket error"),
    });

    let app = Router::new()
        .route("/", get(ui_handler))
        .route("/api/status", get(get_full_status))
        .route("/api/logs/:name", get(get_container_logs))
        .with_state(shared_state);

    let addr = "0.0.0.0:9996";
    println!("🚀 Dashboard running at http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}