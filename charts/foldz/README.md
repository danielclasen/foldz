# foldz chart

Runs foldz, the headless audio downmix over MXL, as a StatefulSet with
`podManagementPolicy: Parallel`, a headless governing Service that publishes
not-ready addresses, Services for the API, NMOS and ops listeners, a
PodDisruptionBudget and a ServiceAccount without an API token. An Ingress, a
NetworkPolicy and a cert-manager Certificate are optional.

The chart and the image of one release carry the same version, and an empty
`image.tag` runs the image tagged with the chart's `appVersion`.

```sh
helm install foldz oci://ghcr.io/danielclasen/foldz/charts/foldz \
  --version <version> --namespace audio -f values.yaml
```

## Configuration

The chart renders one JSON config file into a ConfigMap and starts the
binary with `--config /etc/foldz/config.json`; a change to it rolls the pod.
`config` carries the product section, `inputs` and `outputs`, with the
structure `values.schema.json` gives; both are required, and the
[binary's README](../../README.md#configuration) describes each field.
Everything else in the file comes from the values below, and `config`
refuses to repeat it. Secrets reach the file as paths of mounted Secrets,
never as values.

```yaml
config:
  inputs:
    - name: main
      channels: 8
      layout: [L, R, C, LFE, Ls, Rs, "-", "-"]
      flow: 7a1e4c9b-3d2f-4b6a-8e0c-5f7d9b1a3c55
  outputs:
    - name: stereo
      flows: [0f3a6c2e-5b1d-4e8a-9c7f-2d4b6a8e0c11]
      layout: [L, R]
      preset: bs775-3_2-to-2_0
```

| Value | Default | |
|---|---|---|
| `image.repository`, `.tag`, `.digest` | `ghcr.io/danielclasen/foldz`, `""`, `""` | `tag: ""` is the `appVersion`; a digest wins over the tag |
| `replicas` | `1` | any other value fails rendering: replication is not implemented |
| `witness.enabled` | `false` | `true` fails rendering, for the same reason |
| `mxl.libraryPath` | `/opt/mxl/lib/libmxl.so` | |
| `mxl.domains[]` | `{name: a, hostPath: /dev/shm/mxl/a}` | `name` plus `hostPath` or `volume`; mounted at `/mxl/<name>` |
| `engine.sampleRate`, `.blockSize`, `.readerMarginSamples` | `48000`, `48`, `576` | `maxCatchupBlocks`, `pruneAfterBlocks`, `rampSamples` are accepted too |
| `rt.enabled`, `.priority`, `.rtCpu` | `true`, `70`, `auto` | |
| `nmos.enabled` | `false` | `true` fails rendering: NMOS is not implemented |
| `nmos.port`, `.service.type` | `8081`, `ClusterIP` | the NMOS listener is bound either way |
| `service.type`, `.port` | `ClusterIP`, `8080` | the API listener and its Service |
| `ops.port` | `9090` | `/healthz`, `/readyz`, `/metrics` ([API](../../README.md#api)), always ClusterIP |
| `auth.mode` | `none` | `none`, `token` or `proxy` |
| `auth.token.existingSecret`, `.key` | `""`, `token` | |
| `auth.proxy.userHeader`, `.groupsHeader`, `.groupRoles` | `X-Forwarded-User`, `X-Forwarded-Groups`, `{}` | `groupRoles` maps a group to `viewer` or `operator`; required in `proxy` mode |
| `auth.proxy.sidecar.*` | oauth2-proxy v7.15.5 by digest, port 4180 | `args` and `env` go through `tpl`; `extraArgs` is appended |
| `clusterDomain` | `cluster.local` | the DNS domain of the Services' fully qualified names, which `api.allowedHosts` and the certManager Certificate include |
| `api.allowedHosts`, `.allowedOrigins` | `[]` | added to the binary's defaults, the API Service's names and the ingress hosts |
| `tls.mode` | `none` | `none`, `existingSecret` or `certManager` |
| `tls.listeners` | `[api]` | `api`, `nmos` |
| `tls.existingSecret` | `""` | a `kubernetes.io/tls` Secret |
| `tls.certManager.issuerRef`, `.dnsNames` | `{name: "", kind: Issuer}`, `[]` | |
| `ingress.*` | disabled | `hosts` is a list of names; `tls` is the Ingress's own |
| `networkPolicy.enabled`, `.nmosFrom` | `false`, `[]` | |
| `podDisruptionBudget.enabled`, `.maxUnavailable` | `true`, `1` | |
| `podSecurityContext` | uid and gid 65532, `RuntimeDefault` seccomp | |
| `resources` | 2 CPUs, 512Mi, requests equal to limits | |

Rendering fails, naming the value, on a combination the binary refuses or
that would have no effect.

## MXL domains

Each `mxl.domains[]` entry is mounted at `/mxl/<name>` and appears in the
config's `domains` with that path. `hostPath` mounts a directory that must
exist on the node (`type: Directory`); `volume` takes any other pod volume
source, for example `csi` or `persistentVolumeClaim`. The pod's user and
group, `podSecurityContext.runAsUser`, `runAsGroup` and `fsGroup`, have to be
able to read and write the domain, which with `hostPath` the chart does not
change.

## Real-time scheduling

With `rt.enabled` the container adds `SYS_NICE` and `IPC_LOCK` and allows
privilege escalation, since the binary carries these capabilities in its
permitted file capabilities and `no_new_privs` would stop exec from granting
them. Without it every capability is dropped.

With `rt.rtCpu: auto` the RT thread is pinned only when the pod has
Guaranteed QoS and the CPU request equals the limit in whole CPUs. The chart
enforces this at render time: `resources` needs CPU and memory requests equal
to their limits, and with `auth.mode: proxy` the sidecar's
`auth.proxy.sidecar.resources` needs the same for CPU and memory, since one
Burstable container makes the pod Burstable. Memory values are compared as
written, so use the same notation for request and limit. The binary reads the
CPU request and limit from the downward API. Exclusive CPUs further need the
kubelet's `static` CPU manager policy, which grants them to Guaranteed pods
only. On
kernels built with `CONFIG_RT_GROUP_SCHED`, `SCHED_FIFO` is refused to a
cgroup without an RT runtime budget. The node's own requirements, swap off
and `vm.compact_unevictable_allowed=0`, are in the binary's
[real-time setup](../../README.md#real-time-setup).

Pod Security Admission: the `baseline` and `restricted` levels forbid
`hostPath` volumes and the added capabilities, and `restricted` forbids
privilege escalation. A namespace running foldz with `hostPath` domains or
`rt.enabled` needs the `privileged` level or an exemption. With
`rt.enabled: false` and domains on volume types `restricted` allows, such as
`csi` or `persistentVolumeClaim`, the pod meets `restricted`.

## Authentication

- `none`: no credentials. Refused unless the API Service is `ClusterIP` and
  the ingress is off.
- `token`: a bearer token from `auth.token.existingSecret`, mounted as a
  directory so a rotated Secret reaches the file. Browsers exchange it for a
  session cookie at `POST /api/v1/login`.
- `proxy`: an authenticating proxy runs as a sidecar and the API listener
  binds `127.0.0.1`, so only the sidecar reaches it; the API Service targets
  the sidecar. The default sidecar is oauth2-proxy, given `--http-address`
  and `--upstream`; provider settings go in `auth.proxy.sidecar.extraArgs`
  and its secrets in `envFrom`. The sidecar must strip client-supplied
  copies of the identity headers. `auth.proxy.groupRoles` is required:
  without it the binary gives nobody a role.

The binary checks `Host` on requests without a valid bearer and `Origin` on
WebSocket upgrades. The chart adds the API Service's in-cluster names to
`allowedHosts`, and each ingress host to `allowedHosts` and its origin to
`allowedOrigins` (`https://` when the host is in `ingress.tls`). A name or
origin clients use beyond those goes in `api.allowedHosts` and
`api.allowedOrigins`.

IS-04 and IS-05 carry no authentication. `networkPolicy.nmosFrom` lists the
peers allowed to reach the NMOS port; with the policy enabled and the list
empty, nobody is. The API (or sidecar) and ops ports stay open to every
source.

## TLS

### Behind a TLS-terminating proxy or mesh

`tls.mode: none`. Every listener speaks plain HTTP inside the pod; an
ingress controller, a gateway or a mesh sidecar terminates TLS in front of
it. The binary marks the session cookie `Secure` when the first value of
`X-Forwarded-Proto` is `https`, so the proxy has to set that header. The
browser's `Host` and `Origin` are the external ones, so a name not served by
`ingress.hosts` goes in `api.allowedHosts` and `api.allowedOrigins`.

### Served by foldz

The API listener, and the NMOS listener when `tls.listeners` names it,
serve TLS from `tls.crt` and `tls.key`, re-read every 10 seconds, so a
renewed certificate applies to new connections without a restart. The
Service port is then named `https`. A TLS listener has no plain-HTTP
counterpart: foldz does not redirect HTTP to HTTPS. `auth.mode: proxy`
refuses TLS on the API listener, which only the sidecar reaches.

- `tls.mode: existingSecret` mounts the `kubernetes.io/tls` Secret named
  in `tls.existingSecret`.
- `tls.mode: certManager` creates a cert-manager `Certificate` for the
  in-cluster names of the Services in `tls.listeners` plus
  `tls.certManager.dnsNames`, issued by `tls.certManager.issuerRef` into the
  Secret `<fullname>-tls`, which is `foldz-tls` for a release named `foldz`.
  Rendering fails when the cluster serves no `cert-manager.io/v1`
  `Certificate`.

For use without a proxy in front, expose the API Service as `LoadBalancer`
or `NodePort` with `auth.mode: token`, serve TLS from one of the two modes
above, and list the external name in `api.allowedHosts` and its origin in
`api.allowedOrigins`.

An ingress in front of a TLS-serving API needs the controller's own setting
for an HTTPS backend.

## Replication

Replication is not implemented: the StatefulSet runs one audio replica and
no witness, and `replicas` other than 1 or `witness.enabled` fail rendering.
Each output's `flows` must hold one flow id per replica.

## Tests

`scripts/chart-test.sh` runs `helm lint`, the `helm unittest` suites in
`tests/` (golden renders of every scenario in `ci/` and the validation
rules) and validates each scenario's render with kubeconform against a
pinned Kubernetes API version.

`scripts/chart-smoke.sh <repository>:<tag>` installs the chart with a local
image into a throwaway kind cluster, with RT off, token auth, a `hostPath`
domain and the config of `ci/base.yaml`. It checks `/healthz`, `/readyz`
and that `/api/v1/status` answers 401 without the bearer and 200 with it,
and that the output flow leaves the node's domain on uninstall.
