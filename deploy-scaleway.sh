#!/usr/bin/env bash
set -euo pipefail

# --- Load .env file ---
if [ -f .env ]; then
  echo "==> Loading .env file..."
  set -a
  source .env
  set +a
fi

# --- Configuration (from .env or environment) ---
SCW_REGISTRY_ENDPOINT="${SCW_REGISTRY_ENDPOINT:?Set SCW_REGISTRY_ENDPOINT in .env (e.g. rg.fr-par.scw.cloud/<namespace>)}"
SCW_CONTAINER_ID="${SCW_CONTAINER_ID:?Set SCW_CONTAINER_ID in .env (from terraform output container_id)}"
SERVICE_NAME="${SERVICE_NAME:-firm-remote-mcp}"

IMAGE_TAG=$(git rev-parse --short HEAD)
IMAGE_REF="${SCW_REGISTRY_ENDPOINT}/${SERVICE_NAME}:${IMAGE_TAG}"

echo "==> Deploying ${SERVICE_NAME} (${IMAGE_TAG})"

# --- Authenticate Docker to Scaleway registry ---
echo "==> Logging in to Scaleway registry..."
scw registry login > /dev/null

# --- Build image ---
echo "==> Building image..."
docker build --platform=linux/amd64 -t "${IMAGE_REF}" .

# --- Push image ---
echo "==> Pushing image..."
docker push "${IMAGE_REF}"

# --- Update container to use new image ---
echo "==> Updating container ${SCW_CONTAINER_ID}..."
scw container container update "${SCW_CONTAINER_ID}" registry-image="${IMAGE_REF}" --wait

# --- Print result ---
CONTAINER_ENDPOINT=$(scw container container get "${SCW_CONTAINER_ID}" -o json | python3 -c "import sys,json; print(json.load(sys.stdin)['domain_name'])" 2>/dev/null || echo "<see Scaleway console>")

echo ""
echo "=== Deployment complete ==="
echo "Image:        ${IMAGE_REF}"
echo "Server URL:   https://${CONTAINER_ENDPOINT}"
echo "MCP endpoint: https://${CONTAINER_ENDPOINT}/mcp"
