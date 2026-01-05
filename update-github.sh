#!/bin/bash

# Configuration
REPO="metagenes/monitoring" # Change this to your actual username/repo
ARTIFACT_NAME="mini-pc-monitor-binary"
BINARY_NAME="mini-pc-monitor"
INSTALL_DIR="/usr/local/bin"
SERVICE_NAME="mini-pc-monitor.service"

echo "--- 🔄 Starting Update from GitHub ---"

# 1. Download Artifact
echo "--- ⬇️  Downloading latest artifact... ---"
# Requires gh cli to be installed and authenticated
gh run download -n $ARTIFACT_NAME --repo $REPO --dir ./tmp_update

if [ ! -f "./tmp_update/$BINARY_NAME" ]; then
    echo "❌ Error: Artifact not downloaded or binary name mismatch."
    exit 1
fi

chmod +x ./tmp_update/$BINARY_NAME

# 2. Stop Service
echo "--- 🛑 Stopping Service... ---"
sudo systemctl stop $SERVICE_NAME

# 3. Replace Binary
echo "--- 📂 Replacing Binary... ---"
sudo mv ./tmp_update/$BINARY_NAME $INSTALL_DIR/$BINARY_NAME

# 4. Clean up
rm -rf ./tmp_update

# 5. Start Service
echo "--- 🚀 Starting Service... ---"
sudo systemctl start $SERVICE_NAME

echo "--- ✨ Update Completed! ---"
