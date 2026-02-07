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
PROJECT_ID="${PROJECT_ID:?Set PROJECT_ID in .env or environment}"
REGION="${REGION:-europe-west1}"
SERVICE_NAME="${SERVICE_NAME:-firm-remote-mcp}"
REPO_URL="${REPO_URL:?Set REPO_URL in .env or environment}"
OAUTH_CLIENT_ID="${OAUTH_CLIENT_ID:?Set OAUTH_CLIENT_ID in .env or environment}"
GITHUB_TOKEN="${GITHUB_TOKEN:?Set GITHUB_TOKEN in .env or environment}"
OAUTH_CLIENT_SECRET="${OAUTH_CLIENT_SECRET:?Set OAUTH_CLIENT_SECRET in .env or environment}"
BRANCH="${BRANCH:-mcp}"

echo "==> Deploying ${SERVICE_NAME} to ${PROJECT_ID} (${REGION})"

# --- Enable APIs (idempotent) ---
echo "==> Enabling APIs..."
gcloud services enable \
  run.googleapis.com \
  cloudbuild.googleapis.com \
  secretmanager.googleapis.com \
  --project "${PROJECT_ID}" --quiet

# --- Create or update secrets from .env values ---
upsert_secret() {
  local name="$1"
  local value="$2"
  if gcloud secrets describe "${name}" --project "${PROJECT_ID}" &>/dev/null; then
    echo "    Updating secret '${name}'..."
    printf '%s' "${value}" | gcloud secrets versions add "${name}" \
      --data-file=- \
      --project "${PROJECT_ID}" --quiet
  else
    echo "    Creating secret '${name}'..."
    printf '%s' "${value}" | gcloud secrets create "${name}" \
      --data-file=- \
      --project "${PROJECT_ID}" --quiet
  fi
}

echo "==> Syncing secrets..."
upsert_secret "github-token" "${GITHUB_TOKEN}"
upsert_secret "oauth-client-secret" "${OAUTH_CLIENT_SECRET}"

# --- Build and deploy ---
echo "==> Building and deploying..."
gcloud run deploy "${SERVICE_NAME}" \
  --source . \
  --region "${REGION}" \
  --project "${PROJECT_ID}" \
  --allow-unauthenticated \
  --set-env-vars "REPO_URL=${REPO_URL}" \
  --set-env-vars "BRANCH=${BRANCH}" \
  --set-env-vars "OAUTH_CLIENT_ID=${OAUTH_CLIENT_ID}" \
  --set-secrets "GITHUB_TOKEN=github-token:latest" \
  --set-secrets "OAUTH_CLIENT_SECRET=oauth-client-secret:latest" \
  --max-instances 1 \
  --set-env-vars "RUST_LOG=info" \
  --timeout 300 \
  --quiet

# --- Get service URL and set SERVER_URL ---
SERVICE_URL=$(gcloud run services describe "${SERVICE_NAME}" \
  --region "${REGION}" \
  --project "${PROJECT_ID}" \
  --format "value(status.url)")

echo "==> Setting SERVER_URL=${SERVICE_URL}"
gcloud run services update "${SERVICE_NAME}" \
  --region "${REGION}" \
  --project "${PROJECT_ID}" \
  --set-env-vars "SERVER_URL=${SERVICE_URL}" \
  --quiet

echo ""
echo "=== Deployment complete ==="
echo "Service URL:      ${SERVICE_URL}"
echo "MCP endpoint:     ${SERVICE_URL}/mcp"
echo "OAuth Client ID:  ${OAUTH_CLIENT_ID}"
echo ""
echo "Configure your Claude connector with:"
echo "  Server URL:           ${SERVICE_URL}/mcp"
echo "  OAuth Client ID:      ${OAUTH_CLIENT_ID}"
echo "  OAuth Client Secret:  (the secret you entered above)"
