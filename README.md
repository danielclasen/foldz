# foldz

Headless broadcast audio downmix and channel matrix over MXL, written in
Rust on the [samplez](https://github.com/danielclasen/samplez) engine.
foldz reads 48 kHz audio flows from MXL domains and writes each configured
output to its own MXL flow, at the sample index it read, on libmxl's TAI
timeline. Every output applies one gain matrix to one input: a downmix
preset transcribed from a published standard, a routing of channels by
label or index, or an explicit matrix. One JSON file read at start-up
configures it, and its HTTP API is read-only. How it is built is in
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Features

- Downmix presets from ITU-R BS.775-4, ATSC A/52:2018 and ETSI
  TS 103 190-1, each reporting its citation.
  [Presets and routings](docs/USAGE.md#presets-and-routings)
- `select` and `shuffle` routings by channel label or index, explicit gain
  matrices, a gain per output and optional LFE inclusion.
  [Presets and routings](docs/USAGE.md#presets-and-routings)
- Several inputs, outputs and MXL domains in one process.
  [Configuration](docs/USAGE.md#configuration)
- A read-only API that reports the matrices in use and the engine's state,
  with `/healthz`, `/readyz` and Prometheus `/metrics`.
  [API](docs/USAGE.md#api)
- NMOS IS-04 v1.3 registration and IS-05 v1.2 connections over MXL per
  AMWA BCP-007-03, checked in CI by the AMWA nmos-testing suites.
  [NMOS](docs/USAGE.md#nmos)
- HTTPS from certificate files or a generated certificate, and bearer
  token or authenticating-proxy auth. [HTTPS](docs/USAGE.md#https),
  [Authentication](docs/USAGE.md#authentication)
- A real-time audio thread (`SCHED_FIFO`, locked memory) that runs
  degraded and says so when the privileges are missing.
  [Real-time setup](docs/USAGE.md#real-time-setup)
- A container image, a release binary and a Helm chart.
  [Release binary](docs/USAGE.md#release-binary),
  [charts/foldz](charts/foldz/README.md)

Not in this version: replicated outputs. An output writes one flow.

## Requirements

- Linux x86_64 with glibc 2.34 or later and `libgcc_s.so.1`.
- libmxl v1.1.0 and `libstdc++.so.6`, which the image carries.
- An MXL domain on tmpfs that foldz can write.
- `CAP_SYS_NICE` and `CAP_IPC_LOCK`, or the matching resource limits, for
  the real-time thread; without them foldz runs degraded.

## Quick start

This builds the image from this repository and folds a 5.1 flow to stereo
with it, as the calling user, with the API and ops listeners on localhost.
Releases publish the same image as `ghcr.io/danielclasen/foldz:<version>`.

```sh
docker build -t foldz:local .
mkdir -p /dev/shm/mxl/studio
cat > config.json <<'JSON'
{
  "listeners": { "api": { "bind": "0.0.0.0", "port": 8080 } },
  "domains": [{ "name": "studio", "path": "/mxl/studio" }],
  "inputs": [{
    "name": "programme",
    "channels": 6,
    "layout": ["L", "R", "C", "LFE", "Ls", "Rs"],
    "flow": "5fbec3b1-1b0f-417d-9059-8b94a47197ed"
  }],
  "outputs": [{
    "name": "stereo",
    "flows": ["6fbec3b1-1b0f-417d-9059-8b94a47197ed"],
    "layout": ["L", "R"],
    "preset": "bs775-3_2-to-2_0"
  }]
}
JSON
docker run --rm --name foldz --user "$(id -u):$(id -g)" --read-only --tmpfs /tmp \
  -v /dev/shm/mxl/studio:/mxl/studio \
  -v "$PWD/config.json:/etc/foldz/config.json:ro" \
  -p 127.0.0.1:8080:8080 -p 127.0.0.1:9090:9090 \
  foldz:local
```

In another shell:

```sh
curl -s http://127.0.0.1:9090/readyz
curl -s http://127.0.0.1:8080/api/v1/downmix
curl -s http://127.0.0.1:8080/api/v1/status
ls /dev/shm/mxl/studio
```

`/api/v1/downmix` shows the gain matrix of `stereo` and the table it comes
from. The output flow `6fbec3b1-...` is created at start whether or not a
producer writes the input flow yet; until one does, `/api/v1/status`
reports the input's error and the output carries silence. Ctrl-C stops
foldz and deletes the output flow. This container has no real-time
privileges and reports that in `/api/v1/status` (`rt`).

Other ways to run it:

- The release binary on a host. [Release binary](docs/USAGE.md#release-binary)
- Kubernetes, with the Helm chart each release publishes.
  [charts/foldz](charts/foldz/README.md)

Building, testing, CI and releases are in
[docs/BUILDING.md](docs/BUILDING.md).

## Licence

Apache-2.0. The image carries the licence texts of what it ships:
`/usr/share/licenses/foldz` (foldz and the Rust crates in the binary) and
`/opt/mxl/share/licenses` (libmxl and its dependencies).
