#!/bin/bash
set -euo pipefail

# Only Render's platform-supplied hostname is used. Never interpolate arbitrary
# endpoint URLs, credentials, forwarded addresses or headers into nginx config.
host=${RENDER_EXTERNAL_HOSTNAME:-}
port=${PORT:-10000}
if [[ ! $host =~ ^[a-z0-9]([a-z0-9-]*[a-z0-9])?\.onrender\.com$ ]] ||
   [[ ! $port =~ ^[1-9][0-9]{0,4}$ ]] || (( port > 65535 || port == 8765 )); then
    echo 'Invalid Render hostname or port' >&2
    exit 1
fi
umask 077
config=$(mktemp)
relay_pid=
proxy_pid=
cleanup() {
    trap - EXIT TERM INT
    [[ -z $proxy_pid ]] || kill "$proxy_pid" 2>/dev/null || true
    [[ -z $relay_pid ]] || kill "$relay_pid" 2>/dev/null || true
    wait 2>/dev/null || true
    rm -f "$config"
}
trap cleanup EXIT
trap 'exit 0' TERM INT

cat > "$config" <<EOF
worker_processes 1;
pid /tmp/tide-nginx.pid;
error_log /dev/null;
events { worker_connections 128; }
http {
    access_log off;
    server_tokens off;
    client_max_body_size 32k;
    client_body_buffer_size 32k;
    client_header_buffer_size 1k;
    large_client_header_buffers 2 4k;
    client_header_timeout 3s;
    client_body_timeout 2s;
    send_timeout 3s;
    keepalive_timeout 0;
    reset_timedout_connection on;
    client_body_temp_path /tmp/tide-client-body;
    proxy_temp_path /tmp/tide-proxy;
    fastcgi_temp_path /tmp/tide-fastcgi;
    uwsgi_temp_path /tmp/tide-uwsgi;
    scgi_temp_path /tmp/tide-scgi;
    map \$server_name \$jam_global { default jam; }
    limit_req_zone \$jam_global zone=jam_rate:1m rate=30r/s;
    limit_conn_zone \$jam_global zone=jam_total:1m;
    server {
        listen 0.0.0.0:$port default_server;
        server_name $host;
        location = /healthz {
            default_type text/plain;
            add_header Cache-Control no-store always;
            return 200 'tide-jam-ready-v1';
        }
        location / {
            if (\$http_host != "$host") { return 444; }
            limit_req zone=jam_rate burst=15 nodelay;
            limit_req_status 429;
            limit_conn jam_total 16;
            limit_conn_status 429;
            proxy_pass http://127.0.0.1:8765;
            proxy_http_version 1.1;
            proxy_set_header Host $host;
            proxy_set_header Connection close;
            proxy_set_header Forwarded "";
            proxy_set_header X-Forwarded-For "";
            proxy_set_header X-Forwarded-Host "";
            proxy_set_header X-Forwarded-Proto "";
            proxy_set_header X-Real-IP "";
            # Conservative trial: every visitor shares one quota bucket. Render
            # terminates TLS upstream; do not trust spoofable forwarded IPs.
            proxy_set_header X-Tide-Jam-Peer 127.0.0.1;
            proxy_cache off;
            proxy_buffering off;
            proxy_request_buffering off;
            proxy_connect_timeout 1s;
            proxy_send_timeout 2s;
            proxy_read_timeout 3s;
            proxy_next_upstream off;
            proxy_redirect off;
        }
    }
}
EOF
/usr/local/bin/tide-jam-relay --endpoint "https://$host" --port 8765 &
relay_pid=$!
# Wait for the bounded local backend before exposing readiness.
for attempt in {1..50}; do
    kill -0 "$relay_pid"
    if (echo > /dev/tcp/127.0.0.1/8765) 2>/dev/null; then break; fi
    sleep 0.1
done
(echo > /dev/tcp/127.0.0.1/8765) 2>/dev/null
nginx -t -c "$config"
nginx -c "$config" -g 'daemon off;' &
proxy_pid=$!
# If either process exits, stop its sibling; do not leave a healthy-looking
# proxy running without the relay. Render can then restart the container.
set +e
wait -n "$relay_pid" "$proxy_pid"
exit 1
