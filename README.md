# Mini PC Dashboard (Rust + Axum)

Lightweight dashboard to monitor a mini PC: CPU, RAM, temperature sensors, disk usage, and Docker containers. Backend is Axum + Tokio; frontend uses Tailwind CDN.

## Features
- JSON endpoint `/api/status` exposes CPU usage, RAM used/total, sensor temps, disk usage, and Docker containers.
- Static UI page (`/`) shows live data with a 2s polling interval.
- Disk and sensor stats via `sysinfo`; Docker integration via `bollard`.

## Prerequisites
- Rust toolchain (stable, edition 2024)
- Linux/macOS with access to sensors and disks (sysfs/procfs)
- Docker socket at `/var/run/docker.sock` if you want container status (optional)

## Run Locally
1) Install Rust dependencies (e.g., via `rustup`).
2) From repo root, run:

```bash
cargo run --release
```

The app listens on `http://localhost:9999`.

## Endpoint
- `GET /api/status` → JSON `FullStatus`

Sample response:

```json
{
  "cpu_usage": 12.5,
  "ram_used_mb": 1024,
  "ram_total_mb": 8096,
  "sensors": [["CPU", 54.0]],
  "disks": [{"name": "sda1", "mount_point": "/", "total_gb": 100, "used_gb": 42}],
  "containers": [{"name": "web", "status": "Up 5 minutes", "state": "running"}]
}
```

## Key Files
- Backend and router: `src/main.rs`
- UI page with Tailwind CDN: `src/index.html`
- Crate config: `Cargo.toml`

## Notes
- If no Docker socket is present, the container list will be empty but the server still runs.
- Available temperature sensors depend on hardware/kernel; some machines may not expose data.
