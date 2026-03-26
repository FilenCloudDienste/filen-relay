# Filen Relay

> [!IMPORTANT]
> This project is in active development. **Do not use it.**

Filen Relay provides a convenient way to serve your Filen Drive via WebDAV/HTTP/FTP/SFTP. Access the web interface to create shares mapping to multiple users, and manage basic permissions.

## Installation

### On Your Machine

```bash
docker run -e FILEN_RELAY_ADMIN_EMAIL='your-filen-account@email.com' -p 80:80 ghcr.io/filenclouddienste/filen-relay:main
```

<!-- todo: fix issues here -->

Configuration options (choose one):
- Set `--admin-email` (`FILEN_RELAY_ADMIN_EMAIL`) and `--db-dir` (`FILEN_RELAY_DB_DIR`) options (or environment variables) to create a normal deployment.
- Set `--admin-email` (`FILEN_RELAY_ADMIN_EMAIL`), `--admin-password` (`FILEN_RELAY_ADMIN_PASSWORD`) and `--db-dir` (`FILEN_RELAY_DB_DIR`) to create a deployment where data is stored in the admin's Filen drive. This is useful when the deployments needs to be stateless.
    - You can also instead set `--admin-auth-config` (`FILEN_RELAY_ADMIN_AUTH_CONFIG`) to provide an auth config (containing email, password and API key), which was previously exported from the [Filen CLI](https://github.com/FilenCloudDienste/filen-cli-releases).

> [!WARNING]
> By default, any Filen user is allowed to log into your Filen Relay and create servers. Open "Manage Allowed Users" with your admin account to change this setting.

### In the Public Cloud (Scaleway)

Download the latest Filen Relay Deployer from this project's release page. Execute it in a terminal and follow the instructions to deploy your Filen Relay as a Scaleway Serverless Container, which can scale to zero when not in use. The Deployer has some configuration options (use `--help` to see them). 
