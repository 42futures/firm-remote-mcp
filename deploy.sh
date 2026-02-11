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
ALLOWED_REDIRECT_URIS="${ALLOWED_REDIRECT_URIS:?Set ALLOWED_REDIRECT_URIS in .env or environment}"
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

ensure_secret() {
  local name="$1"
  if gcloud secrets describe "${name}" --project "${PROJECT_ID}" &>/dev/null; then
    echo "    Secret '${name}' exists"
  else
    echo "    Generating secret '${name}'..."
    openssl rand -base64 48 | gcloud secrets create "${name}" \
      --data-file=- \
      --project "${PROJECT_ID}" --quiet
  fi
}

echo "==> Syncing secrets..."
upsert_secret "github-token" "${GITHUB_TOKEN}"
upsert_secret "oauth-client-secret" "${OAUTH_CLIENT_SECRET}"
ensure_secret "jwt-signing-key"

# --- Resolve SERVER_URL ---
if [ -z "${SERVER_URL:-}" ]; then
  # Try querying existing service, fall back to constructing from Cloud Run URL pattern
  SERVER_URL=$(gcloud run services describe "${SERVICE_NAME}" \
    --region "${REGION}" \
    --project "${PROJECT_ID}" \
    --format "value(status.url)" 2>/dev/null || true)
fi
if [ -z "${SERVER_URL:-}" ]; then
  PROJECT_NUMBER=$(gcloud projects describe "${PROJECT_ID}" --format "value(projectNumber)")
  SERVER_URL="https://${SERVICE_NAME}-${PROJECT_NUMBER}.${REGION}.run.app"
fi

echo "==> Using SERVER_URL=${SERVER_URL}"

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
  --set-env-vars "^;;^ALLOWED_REDIRECT_URIS=${ALLOWED_REDIRECT_URIS}" \
  --set-env-vars "SERVER_URL=${SERVER_URL}" \
  --set-env-vars "RUST_LOG=info" \
  ${WORKSPACE_SUBDIR:+--set-env-vars "WORKSPACE_SUBDIR=${WORKSPACE_SUBDIR}"} \
  --set-secrets "GITHUB_TOKEN=github-token:latest" \
  --set-secrets "OAUTH_CLIENT_SECRET=oauth-client-secret:latest" \
  --set-secrets "JWT_SIGNING_KEY=jwt-signing-key:latest" \
  --max-instances 1 \
  --timeout 300 \
  --quiet

echo ""
echo "=== Deployment complete ==="
echo "Server URL:      ${SERVER_URL}"
echo "MCP endpoint:     ${SERVER_URL}/mcp"
echo "OAuth Client ID:  ${OAUTH_CLIENT_ID}"
echo ""
echo "Configure your MCP client with:"
echo "  Server URL:           ${SERVER_URL}/mcp"
echo "  OAuth Client ID:      ${OAUTH_CLIENT_ID}"
echo "  OAuth Client Secret:  ${OAUTH_CLIENT_SECRET}"
