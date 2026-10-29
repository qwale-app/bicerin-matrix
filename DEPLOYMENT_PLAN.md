# DigitalOcean and MongoDB Atlas deployment

This guide deploys one Bicerin instance to a DigitalOcean Droplet, uses a
separately hosted MongoDB Atlas database, terminates TLS with Caddy, and
automatically installs a tested release whenever a GitHub Release is
published.

It intentionally uses an always-on Droplet rather than a platform that sleeps
inactive applications. Bicerin needs durable local files for its server signing
key and media store, and maintains long-lived client sync connections.

## Architecture and prerequisites

The resulting layout is:

```text
Matrix client
    |
    | HTTPS / WebSocket (443)
    v
Caddy on the Droplet
    |
    | HTTP on 127.0.0.1:8448
    v
Bicerin ------ TLS (outbound 27017/443) ------ MongoDB Atlas
    |
    +-- /var/lib/bicerin/signing.key
    +-- /var/lib/bicerin/media
```

Before starting, obtain:

- A domain name, for example `matrix.example.com`.
- A DigitalOcean account and an SSH public key.
- A MongoDB Atlas project and cluster. Choose a production-capable Atlas tier
  for important data; free tiers have service and capacity limits.
- A GitHub repository administrator who can add repository Actions secrets.

Start with a **2 vCPU / 4 GB RAM / 50 GB SSD** Ubuntu 24.04 Droplet. A
2 GB Droplet can be sufficient for Bicerin and a light workload, but 4 GB
provides safer headroom for Rust services, a bridge, Caddy, and operating
system updates. MongoDB runs in Atlas, so it does not consume Droplet memory.

Bicerin is unfederated. It does not need a publicly reachable Matrix
server-to-server port. Expose HTTPS on port 443, not Bicerin's local port
8448.

## 1. Create and secure the Droplet

1. Create an Ubuntu 24.04 Droplet in the region closest to users and the Atlas
   cluster. Add an SSH key during creation and record the Droplet IPv4 address.
2. Create a DigitalOcean Cloud Firewall and attach it to the Droplet:

   | Direction | Protocol / port | Source / destination |
   | --- | --- | --- |
   | Inbound | TCP 22 | Your administrative IP ranges only |
   | Inbound | TCP 80 | All IPv4 and IPv6 |
   | Inbound | TCP 443 | All IPv4 and IPv6 |
   | Outbound | All | All destinations |

   Atlas connections are initiated by the Droplet, so they require outbound
   access. Do not expose port 8448, MongoDB, or a bridge listener publicly.
3. Add a DNS `A` record for `matrix.example.com` pointing at the Droplet IP.
   Wait until it resolves before installing Caddy, because certificate
   issuance needs port 80 and 443 to reach the Droplet.
4. SSH to the Droplet and apply basic OS hardening:

   ```bash
   sudo apt update
   sudo apt upgrade -y
   sudo apt install -y caddy curl rsync ca-certificates

   sudo adduser --disabled-password --gecos "" bicerin
   sudo install -d -o bicerin -g bicerin -m 0750 \
     /opt/bicerin/releases /var/lib/bicerin/media
   ```

5. Disable root password login and password authentication only after verifying
   that SSH-key login works. Keep an out-of-band DigitalOcean console recovery
   path.

## 2. Configure MongoDB Atlas

1. In Atlas, create a database deployment and a database named `bicerin`.
2. Create a dedicated database user for Bicerin. Grant it the `readWrite`
   role on the `bicerin` database. Do not reuse an Atlas project owner or an
   application user from another service.
3. Determine the Droplet's actual outbound IPv4 address:

   ```bash
   curl -4 https://api.ipify.org
   ```

   Add that value as a single-address `/32` entry in Atlas **Network Access**.
   This is preferable to allowing `0.0.0.0/0`. Repeat this step if the
   Droplet's outbound IP changes after a rebuild or migration.
4. From Atlas **Connect**, copy the Node/Rust-compatible `mongodb+srv://`
   connection string. Replace its username, password, and database name.
   URL-encode special characters in the password. Atlas enables TLS for SRV
   connections automatically.

Do not put the connection string in GitHub variables, workflow files, shell
history, or the Git repository. It belongs only in the server configuration.

## 3. Install the Bicerin configuration and service

Create `/etc/bicerin/bicerin.toml`:

```bash
sudo install -d -o root -g bicerin -m 0750 /etc/bicerin
sudo tee /etc/bicerin/bicerin.toml >/dev/null <<'EOF'
[server]
bind = "127.0.0.1:8448"
server_name = "matrix.example.com"
public_url = "https://matrix.example.com"
signing_key_path = "/var/lib/bicerin/signing.key"

[database]
backend = "mongodb"
mongo_uri = "mongodb+srv://BICERIN_USER:URL_ENCODED_PASSWORD@cluster.example.mongodb.net/?retryWrites=true&w=majority"
mongo_database = "bicerin"

[media]
storage_backend = "local"
local_path = "/var/lib/bicerin/media"
max_upload_size_bytes = 52428800

[matrix]
server_name = "matrix.example.com"
registration_enabled = false
guest_access_enabled = false

[rate_limiting]
enabled = true
general_per_minute = 300
sensitive_per_minute = 10
EOF
sudo chown root:bicerin /etc/bicerin/bicerin.toml
sudo chmod 0640 /etc/bicerin/bicerin.toml
```

Replace all example domain, Atlas host, username, and password values before
starting the service. Keep registration disabled after initial setup unless
open registration is intentional. If it is enabled temporarily, use a
registration shared secret and disable it again once the initial accounts
exist.

The current Bicerin media implementation writes to the local path; it does
not yet implement the configured S3-compatible backend. Back up
`/var/lib/bicerin/media` and `/var/lib/bicerin/signing.key` alongside Atlas
database backups.

Create `/etc/systemd/system/bicerin.service`:

```ini
[Unit]
Description=Bicerin Matrix homeserver
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=bicerin
Group=bicerin
WorkingDirectory=/opt/bicerin/current
ExecStart=/opt/bicerin/current/bicerin-server --config /etc/bicerin/bicerin.toml
Restart=on-failure
RestartSec=5
NoNewPrivileges=true
PrivateTmp=true
ProtectHome=true
ProtectSystem=full
ReadWritePaths=/var/lib/bicerin

[Install]
WantedBy=multi-user.target
```

The `current` path will be created during the first deployment. Configure the
restricted restart permission needed by the CI deployment account:

```bash
echo 'bicerin ALL=(root) NOPASSWD: /usr/bin/systemctl restart bicerin, /usr/bin/systemctl is-active --quiet bicerin' |
  sudo tee /etc/sudoers.d/bicerin-deploy >/dev/null
sudo chmod 0440 /etc/sudoers.d/bicerin-deploy
sudo visudo -cf /etc/sudoers.d/bicerin-deploy
sudo systemctl daemon-reload
```

## 4. Configure HTTPS with Caddy

Replace the default Caddy configuration with:

```bash
sudo tee /etc/caddy/Caddyfile >/dev/null <<'EOF'
matrix.example.com {
    reverse_proxy 127.0.0.1:8448
}
EOF
sudo caddy validate --config /etc/caddy/Caddyfile
sudo systemctl reload caddy
```

Caddy obtains and renews the TLS certificate automatically. It also forwards
WebSocket upgrades, which Bicerin's optional WebSocket sync endpoint requires.

## 5. Prepare a dedicated GitHub deployment key

Perform these commands on a trusted administrator workstation, not in a CI
job. The `bicerin` service account has no SSH key immediately after it is
created, so first use the existing Droplet administrator key to install the
deployment public key for that account.

```powershell
ssh-keygen -t ed25519 -f bicerin-deploy -C "bicerin GitHub Actions deploy"

Get-Content .\bicerin-deploy.pub | ssh `
  -i "$env:USERPROFILE\.ssh\bicerin-moka" `
  -o IdentitiesOnly=yes `
  root@DROPLET_IP `
  "sudo install -d -o bicerin -g bicerin -m 0700 /home/bicerin/.ssh; `
   sudo tee /home/bicerin/.ssh/authorized_keys > /dev/null; `
   sudo chown bicerin:bicerin /home/bicerin/.ssh/authorized_keys; `
   sudo chmod 0600 /home/bicerin/.ssh/authorized_keys"

# Specify a broadly supported KEX for older Windows OpenSSH clients.
ssh-keyscan -H -t ed25519 -o KexAlgorithms=curve25519-sha256 DROPLET_IP
```

Verify login with the new key:

```powershell
ssh -i .\bicerin-deploy -o IdentitiesOnly=yes bicerin@DROPLET_IP `
  'id && ls -ld /opt/bicerin/releases'
```

Add these **repository Actions secrets** in GitHub:

| Secret | Value |
| --- | --- |
| `DEPLOY_HOST` | The Droplet IPv4 address or SSH hostname |
| `DEPLOY_SSH_PRIVATE_KEY` | Full contents of `bicerin-deploy` (the private key) |
| `DEPLOY_KNOWN_HOSTS` | The exact hashed host-key line printed by `ssh-keyscan -H` |

Do not run `ssh-keyscan` inside the deployment workflow: doing so would
remove host-identity verification and permit a network-path impersonation
attack. Before saving its output as `DEPLOY_KNOWN_HOSTS`, compare the ED25519
fingerprint against the host key shown in the DigitalOcean console.

## 6. Add the release deployment workflow

Create `.github/workflows/deploy-release.yml` with the following content:

```yaml
name: Deploy published release

on:
  release:
    types: [published]

permissions:
  contents: read

concurrency:
  group: production-deployment
  cancel-in-progress: false

jobs:
  test-build-deploy:
    runs-on: ubuntu-24.04
    services:
      mongodb:
        image: mongo:7
        ports:
          - 27017:27017
        options: >-
          --health-cmd "mongosh --eval 'db.adminCommand({ping: 1})' --quiet"
          --health-interval 10s
          --health-timeout 5s
          --health-retries 10
    env:
      BICERIN_TEST_MONGO_URI: mongodb://127.0.0.1:27017
      DEPLOY_HOST: ${{ secrets.DEPLOY_HOST }}
      DEPLOY_SSH_PRIVATE_KEY: ${{ secrets.DEPLOY_SSH_PRIVATE_KEY }}
      DEPLOY_KNOWN_HOSTS: ${{ secrets.DEPLOY_KNOWN_HOSTS }}
      RELEASE_TAG: ${{ github.event.release.tag_name }}

    steps:
      - uses: actions/checkout@v4
        with:
          ref: ${{ github.event.release.tag_name }}

      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2

      - name: Test release source
        run: cargo test --workspace --locked -- --include-ignored

      - name: Build release binary
        run: cargo build --locked --release -p bicerin-server

      - name: Validate release tag
        shell: bash
        run: |
          [[ "$RELEASE_TAG" =~ ^[0-9A-Za-z._-]+$ ]] ||
            { echo "Release tag contains unsupported deployment-path characters"; exit 1; }

      - name: Configure SSH host verification
        shell: bash
        run: |
          install -d -m 700 "$HOME/.ssh"
          printf '%s\n' "$DEPLOY_SSH_PRIVATE_KEY" > "$HOME/.ssh/id_ed25519"
          chmod 600 "$HOME/.ssh/id_ed25519"
          printf '%s\n' "$DEPLOY_KNOWN_HOSTS" > "$HOME/.ssh/known_hosts"
          chmod 600 "$HOME/.ssh/known_hosts"

      - name: Upload and activate release
        shell: bash
        run: |
          ssh -i "$HOME/.ssh/id_ed25519" -o StrictHostKeyChecking=yes \
            "bicerin@$DEPLOY_HOST" "mkdir -p /opt/bicerin/releases/$RELEASE_TAG"
          rsync -az --chmod=F755 -e "ssh -i $HOME/.ssh/id_ed25519 -o StrictHostKeyChecking=yes" \
            target/release/bicerin-server \
            "bicerin@$DEPLOY_HOST:/opt/bicerin/releases/$RELEASE_TAG/bicerin-server"
          ssh -i "$HOME/.ssh/id_ed25519" -o StrictHostKeyChecking=yes \
            "bicerin@$DEPLOY_HOST" \
            "ln -sfn /opt/bicerin/releases/$RELEASE_TAG /opt/bicerin/current && \
             sudo systemctl restart bicerin && \
             sudo systemctl is-active --quiet bicerin && \
             curl --fail --silent --show-error --max-time 15 \
               http://127.0.0.1:8448/_matrix/client/versions >/dev/null"
```

This workflow is separate from the existing push/pull-request test workflow.
It checks out the immutable tag associated with the published release, runs
the MongoDB-backed test suite, builds Bicerin, uploads the binary over a
host-verified SSH connection, atomically switches the `current` symlink, and
only succeeds after the local Matrix versions endpoint responds.

Commit this workflow through the normal review process. On the first
deployment, publish a release from a tested version tag. GitHub Actions then
creates `/opt/bicerin/releases/<tag>`, points `current` at it, and starts the
system service.

## 7. Validate the first release

On the Droplet:

```bash
sudo systemctl status bicerin --no-pager
sudo journalctl -u bicerin -n 100 --no-pager
curl --fail --show-error http://127.0.0.1:8448/_matrix/client/versions
curl --fail --show-error https://matrix.example.com/_matrix/client/versions
```

The first successful start generates `/var/lib/bicerin/signing.key` and
creates the required MongoDB collections and indexes. Confirm that the
public HTTPS request succeeds before configuring Matrix clients or bridges.

## Operations, rollback, and backups

- **Rollback:** SSH to the Droplet, select a previously known-good release,
  then restart:

  ```bash
  ls -1 /opt/bicerin/releases
  ln -sfn /opt/bicerin/releases/PREVIOUS_TAG /opt/bicerin/current
  sudo systemctl restart bicerin
  curl --fail http://127.0.0.1:8448/_matrix/client/versions
  ```

- **Back up local state:** Back up `/var/lib/bicerin/signing.key` and
  `/var/lib/bicerin/media` to encrypted storage outside the Droplet. The
  signing key must be retained across deployments and restores.
- **Back up Atlas:** Enable Atlas backups appropriate to the selected tier and
  periodically test a database restore. Atlas database backups do not include
  Bicerin media files or the signing key.
- **Monitor:** Alert on the systemd service state, Caddy certificate renewal,
  Droplet disk usage, and failed GitHub deployment workflows.
- **Update access safely:** If an SSH host key or Droplet IP changes, update
  the `DEPLOY_KNOWN_HOSTS` secret from a verified console session before
  publishing another release. Update Atlas Network Access with the new
  verified egress IP before restarting Bicerin.

For appservice bridges, keep their listeners on the same Droplet or a private
network and register their configuration through Bicerin's
`appservices.registrations` setting. Do not publish bridge-admin ports through
Caddy unless the bridge's documentation explicitly requires it.
