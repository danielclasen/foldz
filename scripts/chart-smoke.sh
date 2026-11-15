#!/usr/bin/env bash
# Installs the chart with a locally built image into a throwaway kind
# cluster and checks the pod comes up, the API authenticates and the output
# flow is deleted on uninstall. RT off, token auth, a hostPath MXL domain
# and the config of ci/base.yaml.
# Needs docker, kind, kubectl, helm and curl on PATH.
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

cd "$(dirname "$0")/.."

work=$(mktemp -d)
export KUBECONFIG=$work/kubeconfig
token=$(od -An -tx1 -N32 /dev/urandom | tr -d ' \n')
forwards=()

cleanup() {
  status=$?
  if [ "$status" -ne 0 ] && [ -s "$KUBECONFIG" ]; then
    kubectl -n "$ns" get all,events || true
    kubectl -n "$ns" logs statefulset/foldz --all-containers || true
  fi
  for pid in ${forwards[@]+"${forwards[@]}"}; do kill "$pid" 2>/dev/null || true; done
  kind delete cluster --name "$cluster" || true
  rm -rf "$work"
  exit "$status"
}
trap cleanup EXIT

kind create cluster --name "$cluster" --image "$node_image" --wait 120s
kind load docker-image "$image" --name "$cluster"
node=$cluster-control-plane
# The pod's uid owns the domain, as the chart README requires.
docker exec "$node" install -d -o 65532 -g 65532 "$domain"

kubectl create namespace "$ns"
kubectl -n "$ns" create secret generic foldz-token --from-literal=token="$token"
helm install foldz "$chart" --namespace "$ns" --wait --timeout 3m \
  -f "$chart/ci/base.yaml" \
  --set image.repository="${image%:*}" --set image.tag="${image##*:}" \
  --set image.pullPolicy=Never \
  --set rt.enabled=false \
  --set auth.mode=token --set auth.token.existingSecret=foldz-token \
  --set "mxl.domains[0].name=a" --set "mxl.domains[0].hostPath=$domain"
kubectl -n "$ns" wait --for=condition=Ready pod/foldz-0 --timeout 60s

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
expect 200 "GET /healthz" "http://127.0.0.1:$ops_port/healthz"
expect 200 "GET /readyz" "http://127.0.0.1:$ops_port/readyz"
expect 401 "GET /api/v1/status without a bearer" "http://127.0.0.1:$api_port/api/v1/status"
expect 200 "GET /api/v1/status with the bearer" \
  -H "Authorization: Bearer $token" "http://127.0.0.1:$api_port/api/v1/status"

# The pod writes its output flow into the node's domain directory and
# deletes it when it stops.
docker exec "$node" test -e "$domain/$flow.mxl-flow"
echo "ok output flow $flow is on the node"
helm uninstall foldz --namespace "$ns" --wait --timeout 2m
kubectl -n "$ns" wait --for=delete pod/foldz-0 --timeout 60s
if docker exec "$node" test -e "$domain/$flow.mxl-flow"; then
  echo "FAIL output flow $flow is still on the node after uninstall" >&2
  exit 1
fi
echo "ok output flow $flow is gone after uninstall"
