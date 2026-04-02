# Filen Relay

> [!IMPORTANT]
> This project is in active development. **Expect bugs.**

Filen Relay provides a convenient way to serve your Filen Drive via HTTP/WebDAV/S3. Access the web interface to create shares mapping to multiple users, and manage basic permissions.

## Installation

### 🌐 Filen Relay Deployer (Scaleway)

Download the latest Filen Relay Deployer from the [release page](https://github.com/FilenCloudDienste/filen-relay/releases/latest). Execute it in a terminal and follow the instructions to deploy Filen Relay as a [Scaleway Serverless Container](https://www.scaleway.com/en/serverless-containers/), which can scale to zero when not in use. The Deployer executable has some configuration options (use `--help` to see them). 

### 🖥️ Manual (on your machine)

```bash
docker run -e FILEN_RELAY_ADMIN_EMAIL='your-filen-account@email.com' -e FILEN_RELAY_DB_DIR='/usr/app/data' -v /some/host/path:/usr/app/data -p 80:80 ghcr.io/filenclouddienste/filen-relay:latest
```

Configuration options (choose one):
- Set options (or environment variables) `--admin-email` (`FILEN_RELAY_ADMIN_EMAIL`) and `--db-dir` (`FILEN_RELAY_DB_DIR`) to create a normal deployment.
- Set `--admin-email` (`FILEN_RELAY_ADMIN_EMAIL`), `--admin-password` (`FILEN_RELAY_ADMIN_PASSWORD`) and `--admin-2fa-code` (`FILEN_RELAY_ADMIN_2FA_CODE`) if needed to create a deployment where data is stored in the admin's Filen drive. This is useful when the deployments needs to be stateless.
    - You can also instead set `--admin-auth-config` (`FILEN_RELAY_ADMIN_AUTH_CONFIG`) to provide an auth config (containing email, password and API key), which was previously exported from the [Filen CLI](https://github.com/FilenCloudDienste/filen-cli-releases).
