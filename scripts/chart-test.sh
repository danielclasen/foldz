#!/usr/bin/env bash
# Lints the chart, runs its unit tests and validates every render scenario
# in charts/foldz/ci against the API schemas of one Kubernetes version.
# Needs helm, the helm-unittest plugin and kubeconform on PATH.
set -euo pipefail

chart=charts/foldz
kubernetes_version=${KUBERNETES_VERSION:-1.34.0}
# Both schema sets from pinned commits: the Kubernetes API, and cert-manager's
# Certificate from the CRD catalog.
k8s='https://raw.githubusercontent.com/yannh/kubernetes-json-schema/2bfaff3898a7a4fa4e4760da3d2656211c9b4c71/{{.NormalizedKubernetesVersion}}-standalone{{.StrictSuffix}}/{{.ResourceKind}}{{.KindSuffix}}.json'
crds='https://raw.githubusercontent.com/datreeio/CRDs-catalog/89c736bdde38b5fecd0f8436ec5f3aa3679a3b86/{{.Group}}/{{.ResourceKind}}_{{.ResourceAPIVersion}}.json'

cd "$(dirname "$0")/.."

helm lint --strict "$chart" -f "$chart/ci/base.yaml"
helm unittest "$chart"

for values in "$chart"/ci/*-values.yaml; do
  args=()
  case $values in
    *cert-manager*) args=(--api-versions cert-manager.io/v1/Certificate) ;;
  esac
  echo "== $values"
  helm template foldz "$chart" --namespace foldz \
    -f "$chart/ci/base.yaml" -f "$values" ${args[@]+"${args[@]}"} |
    kubeconform -strict -summary \
      -kubernetes-version "$kubernetes_version" \
      -schema-location "$k8s" -schema-location "$crds"
done
