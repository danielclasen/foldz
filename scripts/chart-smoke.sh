#!/usr/bin/env bash
# Installs the chart with a locally built image into a throwaway kind
# cluster and checks the pod comes up, the API authenticates and the output
# flow is deleted on uninstall, once with RT off and once with RT on and
# rtCpu none, then once with NMOS on: the Node API answers through the
# NMOS Service and an IS-05 activation releases and recreates the output
# flow. Token auth, a hostPath MXL domain and the config of ci/base.yaml.
# Needs docker, kind, kubectl, helm and curl on PATH.
# With KIND_CLUSTER_EXISTS=1 it uses the cluster KIND_CLUSTER names,
# created by the caller from the node image below, and leaves it in place.
set -euo pipefail

image=${1:?usage: scripts/chart-smoke.sh <repository>:<tag> of a local image}
cluster=${KIND_CLUSTER:-foldz-smoke}
# The node image of kind v0.32.0.
node_image=kindest/node:v1.36.1@sha256:3489c7674813ba5d8b1a9977baea8a6e553784dab7b84759d1014dbd78f7ebd5
chart=charts/foldz
ns=foldz
domain=/dev/shm/mxl/a
# The output flow of ci/base.yaml.
flow=0f3a6c2e-5b1d-4e8a-9c7f-2d4b6a8e0c11
api_port=18080
ops_port=19090
nmos_port=18081
mxl_domain_id=5e0c2a1b-7d3f-4e9a-8b6c-1f2e3d4c5b6a

cd "$(dirname "$0")/.."

work=$(mktemp -d)
export KUBECONFIG=$work/kubeconfig
token=$(od -An -tx1 -N32 /dev/urandom | tr -d ' \n')
forwards=()
created=

cleanup() {
  status=$?
  if [ "$status" -ne 0 ] && [ -s "$KUBECONFIG" ]; then
    kubectl -n "$ns" get all,events || true
    kubectl -n "$ns" logs statefulset/foldz --all-containers || true
  fi
  for pid in ${forwards[@]+"${forwards[@]}"}; do kill "$pid" 2>/dev/null || true; done
  # Never delete a cluster this run did not create.
  if [ -n "$created" ]; then kind delete cluster --name "$cluster" || true; fi
  rm -rf "$work"
  exit "$status"
}
trap cleanup EXIT

if [ "${KIND_CLUSTER_EXISTS:-}" = 1 ]; then
  kind export kubeconfig --name "$cluster"
else
  kind create cluster --name "$cluster" --image "$node_image" --wait 120s
  created=1
fi

kind load docker-image "$image" --name "$cluster"
node=$cluster-control-plane
# The pod's uid owns the domain, as the chart README requires.
docker exec "$node" install -d -o 65532 -g 65532 "$domain"

kubectl create namespace "$ns"
kubectl -n "$ns" create secret generic foldz-token --from-literal=token="$token"

# expect <status> <what> <curl args...>; <what> keeps the token out of the log.
expect() {
  local want=$1 what=$2 got
  shift 2
  got=$(curl -s -o /dev/null -w '%{http_code}' "$@")
  if [ "$got" != "$want" ]; then
    echo "FAIL $what: HTTP $got, expected $want" >&2
    return 1
  fi
  echo "ok $what: HTTP $got"
}

# check_install <rt.enabled>: installs the chart, checks the pod and its API,
# uninstalls it and checks the output flow is gone.
check_install() {
  local rt=$1 status
  echo "== helm install with rt.enabled=$rt"
  helm install foldz "$chart" --namespace "$ns" --wait --timeout 3m \
    -f "$chart/ci/base.yaml" \
    --set image.repository="${image%:*}" --set image.tag="${image##*:}" \
    --set image.pullPolicy=Never \
    --set rt.enabled="$rt" --set rt.rtCpu=none \
    --set resources.requests.cpu=500m --set resources.limits.cpu=500m \
    --set auth.mode=token --set auth.token.existingSecret=foldz-token \
    --set "mxl.domains[0].name=a" --set "mxl.domains[0].hostPath=$domain"
  kubectl -n "$ns" wait --for=condition=Ready pod/foldz-0 --timeout 60s

  forwards=()
  kubectl -n "$ns" port-forward svc/foldz "$api_port:8080" >"$work/api.log" 2>&1 &
  forwards+=($!)
  kubectl -n "$ns" port-forward svc/foldz-ops "$ops_port:9090" >"$work/ops.log" 2>&1 &
  forwards+=($!)
  for _ in $(seq 50); do
    if curl -s -o /dev/null "http://127.0.0.1:$api_port/" &&
      curl -s -o /dev/null "http://127.0.0.1:$ops_port/"; then
      break
    fi
    sleep 0.2
  done
  expect 200 "GET /healthz" "http://127.0.0.1:$ops_port/healthz"
  expect 200 "GET /readyz" "http://127.0.0.1:$ops_port/readyz"
  expect 401 "GET /api/v1/status without a bearer" "http://127.0.0.1:$api_port/api/v1/status"
  expect 200 "GET /api/v1/status with the bearer" \
    -H "Authorization: Bearer $token" "http://127.0.0.1:$api_port/api/v1/status"
  status=$(curl -fs -H "Authorization: Bearer $token" "http://127.0.0.1:$api_port/api/v1/status")
  echo "rt steps: $(grep -o '"rt":\[[^]]*\]' <<<"$status")"
  if [ "$rt" = true ]; then
    # The added capabilities are raised by the file capabilities of the
    # binary only if allowPrivilegeEscalation is on.
    grep -q '"name":"capabilities","state":"active"' <<<"$status" \
      || { echo "FAIL capabilities step is not active with rt.enabled" >&2; return 1; }
    for step in mlock sched; do
      grep -q "\"name\":\"$step\",\"state\":\"active\"" <<<"$status" \
        || { echo "FAIL $step step is not active with rt.enabled" >&2; return 1; }
    done
    echo "ok capabilities, mlock and sched steps are active"
  else
    grep -q '"name":"capabilities","state":"active"' <<<"$status" \
      && { echo "FAIL capabilities step is active with rt.enabled=false" >&2; return 1; }
    echo "ok capabilities step is not active"
  fi

  # The pod writes its output flow into the node's domain directory and
  # deletes it when it stops.
  docker exec "$node" test -e "$domain/$flow.mxl-flow"
  echo "ok output flow $flow is on the node"
  for pid in "${forwards[@]}"; do kill "$pid" 2>/dev/null || true; done
  forwards=()
  helm uninstall foldz --namespace "$ns" --wait --timeout 2m
  kubectl -n "$ns" wait --for=delete pod/foldz-0 --timeout 60s
  if docker exec "$node" test -e "$domain/$flow.mxl-flow"; then
    echo "FAIL output flow $flow is still on the node after uninstall" >&2
    return 1
  fi
  echo "ok output flow $flow is gone after uninstall"
}

# check_nmos: installs the chart with NMOS on and no registry, checks the
# Node API, takes one IS-05 sender activation each way and uninstalls.
check_nmos() {
  echo "== helm install with nmos.enabled=true"
  helm install foldz "$chart" --namespace "$ns" --wait --timeout 3m \
    -f "$chart/ci/base.yaml" \
    --set image.repository="${image%:*}" --set image.tag="${image##*:}" \
    --set image.pullPolicy=Never \
    --set rt.enabled=false --set rt.rtCpu=none \
    --set resources.requests.cpu=500m --set resources.limits.cpu=500m \
    --set auth.mode=token --set auth.token.existingSecret=foldz-token \
    --set "mxl.domains[0].name=a" --set "mxl.domains[0].hostPath=$domain" \
    --set "mxl.domains[0].id=$mxl_domain_id" --set nmos.enabled=true
  kubectl -n "$ns" wait --for=condition=Ready pod/foldz-0 --timeout 60s

  forwards=()
  kubectl -n "$ns" port-forward svc/foldz-nmos "$nmos_port:8081" >"$work/nmos.log" 2>&1 &
  forwards+=($!)
  local base=http://127.0.0.1:$nmos_port
  for _ in $(seq 50); do
    curl -s -o /dev/null "$base/x-nmos/" && break
    sleep 0.2
  done
  local sender receiver
  # Controllers reach the node at the NMOS Service.
  curl -fs "$base/x-nmos/node/v1.3/self" | python3 -c '
import json, sys
host, port = sys.argv[1], int(sys.argv[2])
n = json.load(sys.stdin)
assert [(e["host"], e["port"]) for e in n["api"]["endpoints"]] == [(host, port)], n["api"]
' "foldz-nmos.$ns.svc.cluster.local" 8081 \
    || { echo "FAIL the Node does not advertise the NMOS Service and its port" >&2; return 1; }
  echo "ok the Node advertises foldz-nmos.$ns.svc.cluster.local:8081"
  sender=$(curl -fs "$base/x-nmos/node/v1.3/senders/" | python3 -c '
import json, sys
s = json.load(sys.stdin)
assert len(s) == 1 and s[0]["transport"] == "urn:x-nmos:transport:mxl", s
print(s[0]["id"])')
  receiver=$(curl -fs "$base/x-nmos/node/v1.3/receivers/" | python3 -c '
import json, sys
r = json.load(sys.stdin)
assert len(r) == 1 and r[0]["transport"] == "urn:x-nmos:transport:mxl", r
print(r[0]["id"])')
  echo "ok one MXL sender ($sender) and one MXL receiver ($receiver)"
  curl -fs "$base/x-nmos/connection/v1.2/single/receivers/$receiver/active" | python3 -c '
import json, sys
a = json.load(sys.stdin)
assert a["transport_params"][0]["mxl_domain_id"] == sys.argv[1], a
' "$mxl_domain_id" \
    || { echo "FAIL the receiver's domain is not $mxl_domain_id" >&2; return 1; }
  echo "ok the receiver's mxl_domain_id is the configured domain id"

  # activate <master_enable>: an immediate IS-05 activation of the sender.
  activate() {
    expect 200 "PATCH sender master_enable $1" -X PATCH -H 'Content-Type: application/json' \
      -d "{\"master_enable\": $1, \"activation\": {\"mode\": \"activate_immediate\"}}" \
      "$base/x-nmos/connection/v1.2/single/senders/$sender/staged"
  }
  # flow_is <present|absent>: waits up to 10 s for the output flow.
  flow_is() {
    for _ in $(seq 50); do
      if docker exec "$node" test -e "$domain/$flow.mxl-flow"; then
        [ "$1" = present ] && { echo "ok output flow $flow is $1"; return 0; }
      else
        [ "$1" = absent ] && { echo "ok output flow $flow is $1"; return 0; }
      fi
      sleep 0.2
    done
    echo "FAIL output flow $flow is not $1" >&2
    return 1
  }
  flow_is present
  activate false
  flow_is absent
  activate true
  flow_is present

  for pid in "${forwards[@]}"; do kill "$pid" 2>/dev/null || true; done
  forwards=()
  helm uninstall foldz --namespace "$ns" --wait --timeout 2m
  kubectl -n "$ns" wait --for=delete pod/foldz-0 --timeout 60s
}

check_install false
check_install true
check_nmos
