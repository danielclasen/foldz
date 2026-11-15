#!/bin/bash
# Starts the foldz image with a read-only root file system, as the
# calling user three times: with the default capabilities, with every
# capability dropped, and with SYS_NICE and IPC_LOCK added, where the RT
# thread has to run SCHED_FIFO with its memory locked; and once as the
# image's own user, 65532, on a domain it can write. Each time the ops
# listener has to answer /healthz and /readyz, the API listener has to
# serve TLS with the certificate whose fingerprint foldz logged and
# require the bearer token, and the output flow has to appear in the
# domain and be deleted on SIGTERM.
#
# usage: docker/smoke.sh <image>
set -euo pipefail

image=$1
name=foldz-smoke-$$
work=$(mktemp -d /dev/shm/foldz-smoke.XXXXXX)
trap 'docker rm -f "$name" >/dev/null 2>&1 || true; rm -rf "$work"' EXIT

test "$(docker inspect -f '{{.Config.User}}' "$image")" = 65532:65532

in_flow=7a1e4c9b-3d2f-4b6a-8e0c-5f7d9b1a3c55
out_flow=0f3a6c2e-5b1d-4e8a-9c7f-2d4b6a8e0c11
token=$(head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n')
mkdir "$work/domain" "$work/secret"
printf '%s\n' "$token" > "$work/secret/token"
cat > "$work/config.json" <<JSON
{
  "listeners": {
    "api": { "bind": "0.0.0.0", "port": 8080,
             "tls": { "mode": "selfSigned", "sans": ["localhost", "127.0.0.1"] } },
    "ops": { "bind": "0.0.0.0", "port": 9090 }
  },
  "auth": { "mode": "token", "token": { "file": "/run/secrets/foldz/token" } },
  "domains": [{ "name": "a", "path": "/mxl/a" }],
  "inputs": [{ "name": "main", "channels": 6, "flow": "$in_flow",
               "layout": ["L", "R", "C", "LFE", "Ls", "Rs"] }],
  "outputs": [{ "name": "stereo", "flows": ["$out_flow"], "layout": ["L", "R"],
                "preset": "bs775-3_2-to-2_0" }]
}
JSON

# run <expected sched and mlock state> <docker run options...>
run() {
  local rt=$1
  shift
  echo "== foldz with $*"
  docker run -d --name "$name" --read-only --tmpfs /tmp \
    -v "$work/domain:/mxl/a" -v "$work/secret:/run/secrets/foldz:ro" \
    -v "$work/config.json:/etc/foldz/config.json:ro" \
    -p 127.0.0.1::8080 -p 127.0.0.1::9090 "$@" "$image" >/dev/null
  local api ops
  api=$(docker port "$name" 8080/tcp | head -n1)
  ops=$(docker port "$name" 9090/tcp | head -n1)
  # /healthz turns 200 once the audio thread's first blocks are seen.
  for _ in $(seq 50); do
    curl -fsS "http://$ops/healthz" >/dev/null 2>&1 && break
    sleep 0.2
  done
  curl -fsS "http://$ops/healthz"
  echo
  curl -fsS "http://$ops/readyz"
  echo
  curl -fsS "http://$ops/metrics" | grep -E '^samplez_rt_heartbeat_blocks_total [1-9]'

  # TLS with the logged certificate; the bearer is required.
  local logged served
  logged=$(docker logs "$name" 2>&1 | sed -n 's/.*api listener TLS selfSigned: certificate SHA-256 //p')
  served=$(openssl s_client -connect "$api" -servername localhost </dev/null 2>/dev/null \
    | openssl x509 -noout -fingerprint -sha256 | sed 's/.*=//')
  test -n "$logged"
  test "$logged" = "$served"
  test "$(curl -sk -o /dev/null -w '%{http_code}' "https://$api/api/v1/status")" = 401
  local status
  status=$(curl -fsSk -H "Authorization: Bearer $token" "https://$api/api/v1/status")
  for step in sched mlock; do
    grep -q "\"name\":\"$step\",\"state\":\"$rt\"" <<<"$status" \
      || { echo "$step is not $rt: $status"; exit 1; }
  done
  grep -q '"realised":true' <<<"$status"
  test -e "$work/domain/$out_flow.mxl-flow"

  docker stop -t 10 "$name" >/dev/null
  test "$(docker inspect -f '{{.State.ExitCode}}' "$name")" = 0
  test ! -e "$work/domain/$out_flow.mxl-flow"
  docker rm "$name" >/dev/null
}

me=(--user "$(id -u):$(id -g)")
run degraded "${me[@]}"
run degraded "${me[@]}" --cap-drop ALL
run active "${me[@]}" --cap-add SYS_NICE --cap-add IPC_LOCK
chmod 0777 "$work/domain"
run degraded
echo "smoke ok"
