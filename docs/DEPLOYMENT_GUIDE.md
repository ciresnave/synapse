# Synapse Deployment Guide

This guide covers the deployment process for the Synapse neural communication network. Follow these steps to deploy Synapse in production environments.

## Table of Contents

- [System Requirements](#system-requirements)
- [Installation](#installation)
- [Configuration](#configuration)
- [Database Setup](#database-setup)
- [Security Considerations](#security-considerations)
- [Multi-Transport Configuration](#multi-transport-configuration)
- [Scaling](#scaling)
- [Monitoring](#monitoring)
- [Troubleshooting](#troubleshooting)

## System Requirements

### Minimum Requirements

- **CPU**: 2+ cores
- **RAM**: 4GB minimum, 8GB recommended
- **Storage**: 20GB+ SSD
- **OS**: Ubuntu 20.04+, Debian 11+, CentOS 8+, Windows Server 2019+
- **Database**: PostgreSQL 13+
- **Network**: Public internet access for federation

### Recommended Production Setup

- **CPU**: 4+ cores
- **RAM**: 16GB+
- **Storage**: 100GB+ SSD
- **Database**: PostgreSQL 14+ with replication
- **Cache**: Redis 6+
- **Load Balancing**: Nginx or HAProxy

## Installation

### Using Cargo

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# Clone the repository
git clone https://github.com/yourusername/synapse.git
cd synapse

# Build and install
cargo build --release

# Copy the binary to a system path
sudo cp target/release/synapse /usr/local/bin/
```

### Using Docker

```bash
# Pull the latest image
docker pull yourusername/synapse:latest

# Run with environment variables
docker run -d \
  --name synapse \
  -p 8080:8080 \
  -v /path/to/config:/etc/synapse \
  -e DATABASE_URL=postgres://user:password@host:port/database \
  -e SECRET_KEY=your_secret_key \
  yourusername/synapse:latest
```

## Configuration

Synapse uses environment variables and a configuration file for setup:

### Environment Variables

```bash
# Core settings
export SYNAPSE_HOST=0.0.0.0
export SYNAPSE_PORT=8080
export DATABASE_URL=postgres://user:password@localhost/synapse
export REDIS_URL=redis://localhost:6379
export SECRET_KEY=your_secure_secret_key

# Transport settings
export EMAIL_TRANSPORT=true
export EMAIL_SMTP_HOST=smtp.example.com
export EMAIL_SMTP_PORT=587
export EMAIL_USER=synapse@example.com
export EMAIL_PASSWORD=your_email_password

# Logging
export RUST_LOG=info,synapse=debug
```

### Configuration File

Create a `config.toml` file:

```toml
[server]
host = "0.0.0.0"
port = 8080
workers = 4

[database]
url = "postgres://user:password@localhost/synapse"
pool_size = 10
timeout_seconds = 30

[cache]
url = "redis://localhost:6379"
ttl_seconds = 3600

[security]
encryption_key = "your_secure_encryption_key"
token_expiry_hours = 24

[blockchain]
consensus_method = "ProofOfStake"
block_time_seconds = 60
min_validator_stake = 100
trust_decay_rate = 0.01

[transports]
default_urgency = "Interactive"

[transports.email]
enabled = true
smtp_host = "smtp.example.com"
smtp_port = 587
username = "synapse@example.com"
check_interval_seconds = 60

[transports.mdns]
enabled = true
service_name = "synapse"
ttl = 60

[transports.tcp]
enabled = true
port = 9090

[logging]
level = "info"
file = "/var/log/synapse/synapse.log"
rotate_size_mb = 100
keep_files = 10
```

## Database Setup

### Schema Initialization

```bash
# Create the database
psql -c "CREATE DATABASE synapse;"

# Run migrations (automatic when starting Synapse)
# Or manually:
cd /path/to/synapse
cargo run --bin migrate -- up
```

### Recommended PostgreSQL Configuration

Update your `postgresql.conf`:

```
# Connection settings
max_connections = 100
superuser_reserved_connections = 3

# Memory settings
shared_buffers = 1GB  # 25% of RAM
work_mem = 32MB
maintenance_work_mem = 256MB

# Checkpoint settings
min_wal_size = 1GB
max_wal_size = 4GB
checkpoint_completion_target = 0.9
checkpoint_timeout = 15min

# Query planner
random_page_cost = 1.1  # SSD optimized
effective_cache_size = 3GB  # 75% of RAM

# Logging
log_min_duration_statement = 200ms
log_checkpoints = on
log_connections = on
log_disconnections = on
log_lock_waits = on
```

## Security Considerations

### Network Security

- Deploy behind a reverse proxy (Nginx/HAProxy) with TLS
- Configure firewall to restrict access:
  - API endpoint (default: 8080)
  - Direct TCP transport (default: 9090)
  - mDNS service (if used externally)

### Configuration Security

- Store sensitive configuration in environment variables
- Use secrets management for production (AWS Secrets Manager, HashiCorp Vault)
- Rotate encryption keys and credentials regularly

### Sample Nginx Configuration

```nginx
server {
    listen 443 ssl http2;
    server_name synapse.example.com;

    ssl_certificate /etc/letsencrypt/live/synapse.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/synapse.example.com/privkey.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers HIGH:!aNULL:!MD5;
    ssl_prefer_server_ciphers on;
    
    location / {
        proxy_pass http://localhost:8080;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection 'upgrade';
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

## Multi-Transport Configuration

### Email Transport

For email transport to work correctly:

```toml
[transports.email]
enabled = true
smtp_host = "smtp.example.com"
smtp_port = 587
username = "synapse@example.com"
password = "your_password"
check_interval_seconds = 60
imap_host = "imap.example.com"
imap_port = 993
```

### mDNS Transport

For local network discovery:

```toml
[transports.mdns]
enabled = true
service_name = "synapse"
ttl = 60
interface = "default"  # or specify a network interface
```

### TCP Direct Transport

For direct connections:

```toml
[transports.tcp]
enabled = true
port = 9090
bind_address = "0.0.0.0"
connection_timeout_ms = 5000
keep_alive_interval_ms = 30000
```

### WebRTC Transport (for WASM)

For browser communication:

```toml
[transports.webrtc]
enabled = true
stun_servers = ["stun:stun.l.google.com:19302", "stun:stun1.l.google.com:19302"]
turn_servers = []  # Configure for production
ice_transport_policy = "all"  # or "relay" for TURN-only
```

## Scaling

### Horizontal Scaling

Synapse can be horizontally scaled by running multiple instances behind a load balancer:

1. **Shared Database**: All instances connect to the same PostgreSQL database
2. **Shared Cache**: Use Redis for distributed caching
3. **Session Affinity**: Configure sticky sessions if maintaining WebSocket connections

### Load Balancer Configuration (HAProxy)

```
frontend synapse_frontend
    bind *:443 ssl crt /path/to/cert.pem
    default_backend synapse_backend

backend synapse_backend
    balance roundrobin
    cookie SERVERID insert indirect nocache
    option httpchk GET /health
    server synapse1 10.0.0.1:8080 check cookie synapse1
    server synapse2 10.0.0.2:8080 check cookie synapse2
    server synapse3 10.0.0.3:8080 check cookie synapse3
```

## Monitoring

### Health Checks

Synapse provides an endpoint for monitoring:

```
GET /health
```

Response:

```json
{
  "status": "healthy",
  "version": "1.0.0",
  "database": "connected",
  "cache": "connected",
  "transports": {
    "email": "active",
    "mdns": "active",
    "tcp": "active",
    "webrtc": "active"
  },
  "uptime_seconds": 3600,
  "active_connections": 25
}
```

### Metrics

Configure Prometheus metrics with:

```toml
[metrics]
enabled = true
endpoint = "/metrics"
```

Key metrics to monitor:

- `synapse_messages_processed_total`: Total messages processed
- `synapse_message_latency_ms`: Message delivery latency
- `synapse_active_connections`: Number of active connections
- `synapse_http_requests_total`: Total HTTP requests
- `synapse_http_request_duration_seconds`: HTTP request duration

### Logging

Logs are output in JSON format for easier processing by tools like ELK or Graylog:

```
{"timestamp":"2023-05-20T15:32:10.123Z","level":"INFO","message":"Message processed","participant_id":"user@example.com","transport":"email","latency_ms":120}
```

## Troubleshooting

### Common Issues

#### Database Connection Errors

```
Error: Failed to connect to database
```

Check:
- Database server is running
- Connection credentials are correct
- Network connectivity between server and database
- PostgreSQL max_connections limit

#### Transport Initialization Failures

```
Error: Failed to initialize Email transport
```

Check:
- SMTP/IMAP server is accessible
- Credentials are correct
- Required ports are not blocked by firewalls

#### Performance Issues

If experiencing high latency:

1. Check database query performance
2. Increase connection pool size
3. Add or optimize indexes
4. Check Redis cache hit ratio
5. Monitor system resources (CPU, memory, disk I/O)

### Diagnostics

Run diagnostics to check all components:

```bash
synapse diagnose

# Output:
Database: Connected (PostgreSQL 14.5)
Redis Cache: Connected (ping: 2ms)
Email Transport: Connected (SMTP: OK, IMAP: OK)
mDNS Transport: Active (discovered 3 peers)
TCP Transport: Listening on 0.0.0.0:9090
WebRTC Transport: Configured
Trust System: Healthy (last block: 2 minutes ago)
```

## Maintenance

### Backups

Set up regular database backups:

```bash
# Daily backup script
pg_dump -U synapse_user -d synapse > /backup/synapse_$(date +%Y%m%d).sql
```

### Upgrades

To upgrade Synapse:

```bash
# Pull latest changes
git pull

# Build new version
cargo build --release

# Stop service
systemctl stop synapse

# Backup database
pg_dump -U synapse_user -d synapse > /backup/synapse_pre_upgrade.sql

# Replace binary
cp target/release/synapse /usr/local/bin/

# Start service
systemctl start synapse
```

## Support

For additional help:

- GitHub Issues: https://github.com/yourusername/synapse/issues
- Documentation: https://synapse.docs.example.com
- Community Forum: https://forum.example.com/synapse
