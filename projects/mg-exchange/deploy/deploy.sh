#!/usr/bin/env bash
set -euo pipefail

# Deploy exchange to production server
# Usage: ./deploy/deploy.sh [host]

HOST="${1:?Usage: ./deploy/deploy.sh <host>}"
DEPLOY_DIR="/opt/exchange"

echo "=== Deploying to $HOST ==="

# 1. Build images locally
echo "Building Docker images..."
docker build -t exchange:latest .
docker build -t exchange-web:latest clients/web

# 2. Save and transfer images
echo "Saving images..."
docker save exchange:latest exchange-web:latest | gzip > /tmp/exchange-images.tar.gz

echo "Transferring to $HOST..."
scp /tmp/exchange-images.tar.gz "$HOST:$DEPLOY_DIR/"
scp docker-compose.yml "$HOST:$DEPLOY_DIR/"
scp -r deploy/ "$HOST:$DEPLOY_DIR/deploy/"
scp .env.example "$HOST:$DEPLOY_DIR/.env"

# 3. Deploy on remote
echo "Deploying on $HOST..."
ssh "$HOST" bash -s << 'REMOTE'
cd /opt/exchange
docker load < exchange-images.tar.gz
docker compose --profile production down
docker compose --profile production up -d
docker compose ps
echo "=== Deploy complete ==="
REMOTE

rm /tmp/exchange-images.tar.gz
echo "Done."
