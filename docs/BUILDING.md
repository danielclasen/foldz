# Building and testing foldz

How foldz is built, tested and released. [USAGE.md](USAGE.md) describes
how it behaves.

## Build and test

Everything builds and tests inside the samplez builder image, pinned by
digest in `.github/workflows/ci.yml` and the `Dockerfile`; samplez v0.28.0
describes what it holds in
[The builder image](https://github.com/danielclasen/samplez/blob/v0.28.0/docs/BUILDING.md#the-builder-image).
`.github/workflows/ci.yml` has the exact commands. With the builder's
digest as `$BUILDER`:

```sh
docker run --rm --user "$(id -u):$(id -g)" -e SAMPLEZ_REQUIRE_PATHS=avx2 \
  -v "$PWD:/w" -v "$PWD/.cargo-registry:/usr/local/cargo/registry" \
  -v "$PWD/.cargo-git:/usr/local/cargo/git" -w /w "$BUILDER" \
  cargo test --workspace --locked
```

CI runs, in that image, `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets --locked -- -D warnings`, a
separate clippy of the e2e test, and `cargo test --workspace --locked` in
debug and release.

The end-to-end test (`tests/e2e.rs`) runs the binary against libmxl flows
with `SCHED_FIFO` and locked memory. `cargo test` leaves it out; it runs
on its own with the RT limits raised, in debug and release:

```sh
docker run --rm --user "$(id -u):$(id -g)" --ulimit rtprio=99 --ulimit memlock=-1 \
  -e SAMPLEZ_REQUIRE_PATHS=avx2 -e SAMPLEZ_REQUIRE_RT=1 \
  -v "$PWD:/w" -v "$PWD/.cargo-registry:/usr/local/cargo/registry" \
  -v "$PWD/.cargo-git:/usr/local/cargo/git" -w /w "$BUILDER" \
  cargo test --locked --test e2e
```

## The image

```sh
docker build -t foldz:local .
docker/smoke.sh foldz:local
```

The `Dockerfile` builds the binary in the builder image, sets its file
capabilities `cap_sys_nice,cap_ipc_lock=p`, and copies it with libmxl and
the licence texts onto a Debian base that runs as uid and gid 65532. Its
`dist` target holds only the binary and `THIRD-PARTY.txt`, the files a
release attaches.

`docker/smoke.sh` starts the image with a read-only root file system, as
the calling user, three times: with the default capabilities, with every
capability dropped, and with `SYS_NICE` and `IPC_LOCK` added, where the RT
thread has to run `SCHED_FIFO` with its memory locked; and once as uid
65532. Each time `/healthz` and `/readyz` have to answer, the API has to
serve TLS with the certificate whose fingerprint foldz logged and require
the bearer token, and the output flow has to appear and be deleted on
SIGTERM.

## NMOS conformance

`tests/nmos/conformance.sh` runs the AMWA
[nmos-testing](https://github.com/AMWA-TV/nmos-testing) suites IS-04-01
(unicast and multicast DNS-SD), IS-05-01 and BCP-007-03-01 against the
image, each against a freshly started container, and fails when a restart
changed a resource id. The tool validates against the specification
commits in `tests/nmos/specs.txt`. Absolute activations need the kernel's
TAI offset set, which `tests/nmos/set-tai-offset.py` does in CI.

`tests/nmos/gate.py` fails on any Fail, Warning or Not Implemented and on
any other result not listed with its reason in `tests/nmos/expected.json`.
The listed ones:

| Suite | Tests | Result | Why |
|---|---|---|---|
| IS-04-01 | `test_01`, `test_01_01` (unicast run), `test_02`, `test_02_01` (multicast run), `test_12` | Test Disabled | the other DNS-SD mode; disabled for Nodes from v1.3 |
| IS-04-01 | `test_13`, `test_14` | Could Not Test | the tool exercises RTP receivers only |
| IS-04-01 | `test_19`, `test_19_01` | Could Not Test | MXL ports have no interface bindings, so the Node lists no interfaces |
| IS-04-01 | `test_22` | Manual | `conformance.sh` compares every resource id across the restarts |
| IS-05-01 | `test_01` to `test_08`, `test_33` | Not Applicable | replaced by the `auto` tests |
| IS-05-01 | `test_09_01`, `test_11`, `test_11_02`, `test_12`, `test_12_02`, `test_15`, `test_16`, `test_41`, `test_42` | Could Not Test | MXL senders and receivers are covered by BCP-007-03-01 |
| BCP-007-03-01 | `test_15` | Manual | `tests/e2e.rs` releases and recreates a sender's flow |
| all | `auto_node_17` to `_23`, `auto_connection_23` to `_29` | Test Disabled | authorization checks, IS-10 is out of scope |

## The chart

`scripts/chart-test.sh` runs `helm lint`, the `helm unittest` suites in
`charts/foldz/tests` (golden renders of every scenario in
`charts/foldz/ci` and the validation rules), and validates each scenario's
render with kubeconform against a pinned Kubernetes API version. It needs
helm, the helm-unittest plugin and kubeconform.

`scripts/chart-smoke.sh <repository>:<tag>` installs the chart with a
local image into a throwaway kind cluster, with token auth, a `hostPath`
domain and the config of `charts/foldz/ci/base.yaml`: once with RT off,
once with RT on and `rtCpu: none`, and once with NMOS on. It checks
`/healthz`, `/readyz`, that `/api/v1/status` answers 401 without the
bearer and 200 with it, and that the output flow leaves the node's domain
on uninstall; with NMOS on, that the Node API answers through the NMOS
Service and that an IS-05 activation releases and recreates the output
flow. It needs docker, kind, kubectl, helm and curl.

## CI

`.github/workflows/ci.yml` runs on every pull request and on `main`:

- `ci`: the format, lint and test runs above and the e2e test;
- `image`: the image and its smoke test, and a check that the release
  binary runs;
- `nmos`: the conformance suites and their gate;
- `chart`: `scripts/chart-test.sh`;
- `chart-smoke`: `scripts/chart-smoke.sh` against the image.

## Releases

release-please cuts releases from Conventional Commits; merging the
release PR it opens cuts the release and bumps `Cargo.toml` and
`charts/foldz/Chart.yaml` to the same version.
`.github/workflows/release-please.yml` then publishes:

- the image, `ghcr.io/danielclasen/foldz:<version>`;
- the release assets `foldz-<version>-x86_64-linux`,
  `foldz-<version>-THIRD-PARTY.txt` and `foldz-<version>-SHA256SUMS`,
  built from the `dist` target and checked to be the published image's
  binary;
- the chart, at the same version, under the OCI address
  [charts/foldz](../charts/foldz/README.md) installs from.
