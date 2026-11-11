#!/usr/bin/env bash
# Once (safe to run again): the tools for the newer jobs. Python with OpenCV (photos.sh) and a small language
# model (similar.sh) in $DATA_DIR/venv, and Docker for the restore drill and the preview site.
# About 1.5 GB of disk. Then the cron lines: bash ops/install-cron.sh (update.sh does this nightly anyway).
set -uo pipefail
OPS="$(cd "$(dirname "$0")" && pwd)"
DATA="$( { grep -E '^DATA_DIR=.+' "$OPS/env" 2>/dev/null || true; } | tail -1 | cut -d= -f2- | tr -d "\"'\r")"; [ -n "$DATA" ] || DATA="$HOME/bbv"
echo "--- system packages (python venv, docker)"
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq python3-venv python3-pip docker.io >/dev/null 2>&1 || echo "apt had a problem; carrying on"
sudo usermod -aG docker "$USER" 2>/dev/null || true
echo "--- python tools in $DATA/venv"
[ -x "$DATA/venv/bin/python" ] || python3 -m venv "$DATA/venv"
"$DATA/venv/bin/pip" install -q --upgrade pip
# CPU-only PyTorch: the GPU build is several GB and this VM has no GPU.
"$DATA/venv/bin/pip" install -q --index-url https://download.pytorch.org/whl/cpu torch
"$DATA/venv/bin/pip" install -q sentence-transformers opencv-python-headless numpy
"$DATA/venv/bin/python" -c "import cv2, sentence_transformers; print('  opencv', cv2.__version__, '· sentence-transformers', sentence_transformers.__version__)"
echo "--- the language model (downloaded once, about 90 MB)"
HF_HOME="$DATA/hf" "$DATA/venv/bin/python" -c "from sentence_transformers import SentenceTransformer; SentenceTransformer('sentence-transformers/all-MiniLM-L6-v2', device='cpu'); print('  model ready')"
echo "--- cron"
bash "$OPS/install-cron.sh"
echo "DONE. Docker works for cron jobs straight away; for this SSH window, log out and in again."
