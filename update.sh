#!/bin/bash

# Tentukan folder proyek
PROJECT_DIR="/home/yudiminipc/belajar-rust"
BINARY_NAME="mini-pc-monitor"
SERVICE_NAME="mini-pc-monitor.service"

echo "--- 🔄 Memulai Update Dashboard ---"

# 1. Masuk ke direktori
cd $PROJECT_DIR || exit

# 2. Tarik kode terbaru dari Git (opsional)
# git pull origin master

# 3. Kompilasi ulang mode Release
echo "--- 🦀 Mengompilasi dalam mode Release... ---"
cargo build --release

if [ $? -eq 0 ]; then
    echo "--- ✅ Kompilasi Berhasil! ---"

    # 4. Update binary di folder sistem
    echo "--- 📂 Memindahkan binary ke /usr/local/bin/ ---"
    sudo cp target/release/belajar-rust /usr/local/bin/$BINARY_NAME

    # 5. Restart service
    echo "--- 🚀 Merestart Service Systemd... ---"
    sudo systemctl restart $SERVICE_NAME

    echo "--- ✨ Update Selesai! Dashboard sudah kembali online. ---"
else
    echo "--- ❌ Kompilasi Gagal. Update dibatalkan. ---"
    exit 1
fi