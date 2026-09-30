# Bicerin Matrix

Bicerin is an early-stage, purpose-built [Matrix](https://matrix.org) homeserver written in Rust. Unlike general-purpose homeservers (e.g. Synapse, Dendrite, Conduit), Bicerin is **deliberately narrow**: it implements a single-tenant, **unfederated** Matrix homeserver optimized for using Matrix bridges. It is designed to make using Matrix bridges as easy as possible without requiring unnecessary Matrix server-to-server federation bloat.

Bicerin was inspired by Beeper's proprietary hungryserv Matrix server used for their commercial Beeper app.

## Why "unfederated"?

Federation is the single biggest source of complexity in a general Matrix homeserver: state resolution across servers you don't trust, retry queues, signing, backfill, ACLs, etc. Bicerin removes all of that by assuming every room, user, and device is local to the server. What's left is a much simpler core, that significantly improves performance and ease-of-use for bridge-heavy applications.

## Storage backends

Bicerin's storage layer (`bicerin-storage`) is backend-agnostic: it supports both **PostgreSQL** and **MongoDB**. Pick one per deployment via the `database.backend` setting; nothing else about the API or setup changes.

## Project layout

```text
Cargo.toml               # workspace definition
migrations/               # SQL schema for the Postgres backend (run automatically on startup)
crates/
  bicerin-server/          # binary: config load, DB pool, migrations, HTTP server startup
  bicerin-http/             # Axum routes for the Matrix Client-Server API
  bicerin-auth/             # access tokens, password hashing, auth middleware
  bicerin-rooms/            # room creation, membership, state, power levels
  bicerin-events/           # event validation, persistence, relations
  bicerin-sync/             # /sync token handling, filters, room-update pub/sub
  bicerin-media/            # local-disk media upload/download
  bicerin-storage/          # storage abstraction (Store) with PostgreSQL (sqlx) and MongoDB backends
  bicerin-types/             # shared/Ruma-based ID and event types
  bicerin-config/           # configuration loading (file + env vars)
  bicerin-error/            # shared error type + Matrix-style error responses
```

## Prerequisites

- **Rust** (stable), installed via [rustup](https://rustup.rs/).
  - On Windows this also requires the MSVC build tools (Visual Studio Build Tools with the "Desktop development with C++" workload) so the linker is available.
- A datastore — either:
  - **PostgreSQL 14+**, reachable from the machine running Bicerin, or
  - **MongoDB 6+**, self-hosted or externally hosted (e.g. MongoDB Atlas).
- A compatible **Matrix client** to test with. (Note that many clients will not work, because Bicerin is a partial implementation of the Matrix spec, focused on using Matrix bridges to connect with external chat apps through puppeting)

## Setup

### 1. Install Rust (if needed)

```powershell
winget install --id Rustlang.Rustup -e
```

Then restart your terminal (or add `%USERPROFILE%\.cargo\bin` to `PATH`) so
`cargo` and `rustc` are available.

### 2. Create a database

Pick one backend:

**PostgreSQL** (default):

```sql
CREATE DATABASE bicerin;
```

The default connection string (`postgres://postgres:postgres@localhost:5432/bicerin`)
assumes a local Postgres with that user/password — adjust as needed, see
[Configuration](#4-configure) below.

**MongoDB** (self-hosted or externally hosted, e.g. Atlas): no schema to
create ahead of time — Bicerin creates the database, collections, and indexes
automatically on first startup. You just need a connection string (see below).

### 3. Build

From the repository root:

```powershell
cargo build --workspace
```

### 4. Configure

Bicerin is configured via environment variables prefixed with `BICERIN__`
(double underscore separates sections from fields), or an optional config
file. The minimum you'll usually want to set:

**PostgreSQL** (default backend):

```powershell
$env:BICERIN__SERVER__SERVER_NAME = "localhost"
$env:BICERIN__DATABASE__URL = "postgres://postgres:postgres@localhost:5432/bicerin"
$env:BICERIN__MATRIX__REGISTRATION_ENABLED = "true"
```

**MongoDB** instead (self-hosted or an externally-hosted cluster like Atlas):

```powershell
$env:BICERIN__SERVER__SERVER_NAME = "localhost"
$env:BICERIN__DATABASE__BACKEND = "mongodb"
$env:BICERIN__DATABASE__MONGO_URI = "mongodb+srv://user:password@cluster0.example.mongodb.net"
$env:BICERIN__DATABASE__MONGO_DATABASE = "bicerin"
$env:BICERIN__MATRIX__REGISTRATION_ENABLED = "true"
```

(Registration is **disabled by default** — you must opt in explicitly.)

### 5. Run

```powershell
cargo run -p bicerin-server
```

On startup, Bicerin connects to the configured datastore and prepares its
schema — SQL migrations from `migrations/` for Postgres, or index/counter
setup for MongoDB — then starts listening (default `0.0.0.0:8448`).

Optionally point at a config file instead of/in addition to env vars:

```powershell
cargo run -p bicerin-server -- --config .\bicerin.toml
```

## Quick smoke test

Once the server is running (defaults below assume `localhost:8448`):

```powershell
# Check the server is up and see which spec versions it advertises
curl http://localhost:8448/_matrix/client/versions

# Register a user (registration must be enabled, see above)
curl -X POST http://localhost:8448/_matrix/client/v3/register `
  -H "Content-Type: application/json" `
  -d '{\"username\":\"alice\",\"password\":\"correct horse battery staple\"}'
# -> 401 with a UIA challenge; re-send with "auth": {"type": "m.login.dummy"}
```
