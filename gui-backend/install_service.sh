#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SERVICE_NAME="ghost-gui"
SERVICE_FILE="$SCRIPT_DIR/$SERVICE_NAME.service"
SYSTEMD_DIR="/etc/systemd/system"

echo "=== Ghost GUI — instalacja serwisu systemd ==="

# 0. Włącz linger dla roota — procesy (tmux, ghost-launcher) przeżyją
#    rozłączenie SSH nawet gdy systemd-logind ma KillUserProcesses=yes.
echo "[0/4] Włączanie loginctl linger dla root..."
loginctl enable-linger root 2>/dev/null || echo "      (pominięto — brak loginctl lub brak uprawnień)"

# 1. Buduj binarkę
echo "[1/5] Budowanie ghost-gui (release)..."
cd "$WORKSPACE_ROOT"
cargo build --release --bin ghost-gui

BINARY="$WORKSPACE_ROOT/target/release/ghost-gui"
if [[ ! -f "$BINARY" ]]; then
    echo "BŁĄD: Binarka $BINARY nie istnieje po buildzie!"
    exit 1
fi
echo "      Binarka: $BINARY"

# 2. Kopiuj unit file
echo "[2/5] Instalacja unit file: $SYSTEMD_DIR/$SERVICE_NAME.service"
cp "$SERVICE_FILE" "$SYSTEMD_DIR/$SERVICE_NAME.service"

# 3. Przeładuj systemd
echo "[3/5] Przeładowanie systemd..."
systemctl daemon-reload

# 4. Włącz i uruchom
echo "[4/5] Włączanie i uruchamianie $SERVICE_NAME..."
systemctl enable "$SERVICE_NAME"
systemctl restart "$SERVICE_NAME"

echo "[5/5] Gotowe."

echo ""
echo "=== Gotowe! ==="
echo "  Status:   systemctl status $SERVICE_NAME"
echo "  Logi:     journalctl -u $SERVICE_NAME -f"
echo "  Panel:    http://$(hostname -I | awk '{print $1}'):8800"
echo ""
