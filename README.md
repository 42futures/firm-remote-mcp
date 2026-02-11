# Firm Remote MCP

A lightweight remote [MCP](https://modelcontextprotocol.io/) server for [Firm](https://github.com/42futures/firm) workspaces, designed to execute on Google Cloud Run.

This is a proof-of-concept showing how to give AI agents remote access to a Firm workspace backed by a Git repository. It wraps the tools from the `firm_mcp` crate and adds Git-based persistence and OAuth authentication on top. It is not intended for production use or larger teams. Think of it as a starting point you can adapt.

## How it works

The server uses a Git repository as its data layer:

1. **On startup**, the server clones the repository and checks out a dedicated branch (default: `mcp`), merging in the latest from `main`.
2. **Read operations** (list, get, query, search, etc.) work against the local clone without modifying anything.
3. **Write operations** (add entity, write/replace/delete source) validate the change, apply it, and create an atomic commit pushed to the `mcp` branch.
4. **After a period of inactivity**, Cloud Run scales the instance to zero. The next request brings it back, starting from a fresh clone.

This means your agents can read and modify your workspace safely: changes land on a separate branch that you review and merge at your own pace.

## Limitations

- **Single-user.** There's no conflict handling. If the local state diverges from the remote, the server resets to match origin. Not suitable for concurrent use by multiple people.
- **Stateless infrastructure.** Auth codes and session state live in memory. When the instance scales to zero, they're lost. JWTs survive because they're self-contained.
- **No dynamic client registration.** The server uses pre-configured OAuth credentials. Your MCP client must be given the client ID and secret directly.
- **Basic security hardening.** Constant-time credential comparison, PKCE enforcement, redirect URI validation, JWT signing. Adequate for personal/small-team use, not vetted for high-security environments.

## Setup

### Prerequisites

- A GCP project with billing enabled
- `gcloud` CLI installed and authenticated
- A GitHub repository containing your Firm workspace
- A GitHub personal access token with repo access

### Configuration

Copy `.env.example` to `.env` and fill in the values:

```sh
cp .env.example .env
```

| Variable | Description |
|---|---|
| `PROJECT_ID` | Your GCP project ID |
| `REGION` | GCP region (default: `europe-west1`) |
| `SERVER_URL` | The server's public URL (we try to detect it first deploy if omitted) |
| `REPO_URL` | HTTPS URL of your Firm workspace repository |
| `BRANCH` | Branch for MCP commits (default: `mcp`) |
| `GITHUB_TOKEN` | GitHub PAT with repo access |
| `OAUTH_CLIENT_ID` | You choose this, then give it to your MCP client |
| `OAUTH_CLIENT_SECRET` | You choose this, then give it to your MCP client |
| `JWT_SIGNING_KEY` | Key for signing tokens (auto-generated on first deploy if omitted) |
| `ALLOWED_REDIRECT_URIS` | Comma-separated OAuth callback URLs for your MCP client |
| `WORKSPACE_SUBDIR` | Subdirectory within the repo containing the workspace (optional) |

For Claude, set the redirect URIs to:
```
ALLOWED_REDIRECT_URIS=https://claude.ai/api/mcp/auth_callback,https://claude.com/api/mcp/auth_callback
```

### Deploy

```sh
./deploy.sh
```

The script:
- Enables the Cloud Run, Cloud Build, and Secret Manager APIs
- Stores `GITHUB_TOKEN` and `OAUTH_CLIENT_SECRET` in Secret Manager
- Generates a `JWT_SIGNING_KEY` in Secret Manager if one doesn't exist
- Builds the container and deploys it to Cloud Run

On completion it prints the server URL and the credentials you'll need for your MCP client.

## Connecting your MCP client

In Claude (or another MCP client that supports remote servers with OAuth), add a custom connector with:

| Setting | Value |
|---|---|
| Server URL | `https://<your-service>.run.app/mcp` |
| Client ID | The `OAUTH_CLIENT_ID` you configured |
| Client Secret | The `OAUTH_CLIENT_SECRET` you configured |

The server implements the OAuth 2.0 authorization code flow with PKCE. On first connection, your client will complete the OAuth handshake automatically.

## License

AGPL-3.0
