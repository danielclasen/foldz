# foldz

Headless broadcast audio downmix and channel matrix over MXL, built on the
[samplez](https://github.com/danielclasen/samplez) engine, written in Rust.

foldz reads MXL audio flows, applies one gain matrix per output (a cited
downmix preset, a routing or an explicit matrix) and writes each output
to its own MXL flow at the sample index it read, on libmxl's TAI
timeline. Shape, layouts and matrices are fixed at start-up; the HTTP API
is read-only.

Not in this version: NMOS (`nmos.enabled: true` is refused), replicated
outputs (more than one entry in an output's `flows` is refused), meters,
loudness and over counts.

## Presets

Channel labels are the IS-04 v1.3 `source_audio.json` channel symbols,
plus `-` for an unused channel.

Coefficients are transcribed from the cited tables and applied without
headroom scaling; `gainDb` per output scales the whole matrix.

| Preset | Source |
|---|---|
| `bs775-3_2-to-1_0`, `-2_0`, `-3_0`, `-2_1`, `-3_1`, `-2_2` | ITU-R BS.775-4 (12/2022), Annex 4, Table 2 |
| `a52-loro-clev<c>-slev<s>`, `c` one of `0.707`, `0.595`, `0.500`, `s` one of `0.707`, `0.500`, `0` | ATSC A/52:2018, clause 7.8.2, Tables 5.9 and 5.10 |
| `a52-ltrt` | ATSC A/52:2018, clause 7.8.2 |
| `ac4-7_1-to-5_1` | ETSI TS 103 190-1 V1.4.1, clause 6.2.17.5, Table 219, row 3/4/0.x; input `[L, R, C, LFE, Lss, Rss, Lrs, Rrs]`, AC-4 side surrounds read `Lss`/`Rss` and back surrounds `Lrs`/`Rrs` |
| `ac4-bs775-7_1-to-2_0` | `ac4-7_1-to-5_1` followed by `bs775-3_2-to-2_0`, the product of the two matrices |

`select` copies input channels by label, `shuffle` by 0-based index from
`source`, and `matrix` takes an explicit gain matrix with one row per output
channel. `lfe: include` adds the input's LFE to 1/0 and 2/0 outputs at its own
`gainDb`, which the output's `gainDb` then scales like every other gain,
so LFE arrives at the sum of the two; LFE is dropped otherwise.

## Running it standalone

foldz runs on Linux x86_64 and loads libmxl v1.1.0 (`dmf-mxl/mxl`, Release
build) from `mxl.libraryPath` at start. The image carries that build at
`/opt/mxl/lib`:

```sh
id=$(docker create ghcr.io/danielclasen/foldz:<version>)
docker cp -L "$id:/opt/mxl/lib/libmxl.so" ./libmxl.so
docker cp "$id:/opt/mxl/share/licenses" ./mxl-licenses
docker rm "$id"
```

The MXL domain is a directory, normally on `/dev/shm`, shared by every
process that reads or writes its flows; foldz needs write access to it.

### HTTPS

Each listener serves plain HTTP unless its `tls.mode` says otherwise.

A generated certificate, for a host without one:

```json
"listeners": {
  "api": { "bind": "0.0.0.0", "port": 8443,
           "tls": { "mode": "selfSigned", "sans": ["foldz.example", "192.0.2.10"] } }
}
```

It is generated at each start and kept in memory only, valid for a year
from an hour before it is generated and replaced at the first reload
within 30 days of its expiry. foldz logs its SHA-256 fingerprint at start
(`foldz: api listener TLS selfSigned: certificate SHA-256 ...`) and
reports it in `/api/v1/status`; compare it with what a client sees:

```sh
openssl s_client -connect foldz.example:8443 </dev/null 2>/dev/null \
  | openssl x509 -noout -fingerprint -sha256
```

A certificate from files:

```json
"tls": { "mode": "files", "certFile": "/etc/foldz/tls/cert.pem",
         "keyFile": "/etc/foldz/tls/key.pem" }
```

`certFile` is PEM, the server certificate first, then its chain; `keyFile`
is PEM in PKCS#8, PKCS#1 or SEC1. Both are re-read every 10 s and a
changed pair applies from the next handshake; a pair that fails to load
keeps the previous one and counts in
`samplez_secret_reload_failures_total`. With `clientCaFile` (PEM) set, a
client must present a certificate issued by one of those CAs.

### Authentication

`auth.mode` is `none`, `token` or `proxy`. With `token`, every API request
needs the bearer token from `auth.token.file`, read without trailing
whitespace and re-read every 10 s:

```sh
umask 077; openssl rand -hex 32 > /etc/foldz/token
curl --cacert cert.pem -H "Authorization: Bearer $(cat /etc/foldz/token)" \
  https://foldz.example:8443/api/v1/downmix
```

`POST /api/v1/login` with the token exchanges it for a session cookie, for
browsers. In `proxy` mode the API listener binds `127.0.0.1` only and
takes the user and groups from the headers of an authenticating proxy in
front (`auth.proxy.userHeader`, default `X-Forwarded-User`;
`auth.proxy.groupsHeader`, default `X-Forwarded-Groups`, comma-separated);
`auth.proxy.groupRoles` maps group names to `viewer` or `operator` and
is required. Requests without a valid bearer must carry a `Host` of
`localhost`, `127.0.0.1` or a name in `api.allowedHosts`.

The ops listener (`/healthz`, `/readyz`, `/metrics`) is not authenticated.

### A complete example

`/etc/foldz/config.json`, a 5.1 flow folded to stereo with BS.775-4:

```json
{
  "mxl": { "libraryPath": "/opt/mxl/lib/libmxl.so" },
  "listeners": {
    "api": { "bind": "0.0.0.0", "port": 8443,
             "tls": { "mode": "files", "certFile": "/etc/foldz/tls/cert.pem",
                      "keyFile": "/etc/foldz/tls/key.pem" } },
    "ops": { "bind": "127.0.0.1", "port": 9090 }
  },
  "auth": { "mode": "token", "token": { "file": "/etc/foldz/token" } },
  "api": { "allowedHosts": ["foldz.example"] },
  "domains": [{ "name": "studio", "path": "/dev/shm/mxl/studio" }],
  "inputs": [{
    "name": "programme",
    "channels": 6,
    "layout": ["L", "R", "C", "LFE", "Ls", "Rs"],
    "flow": "5fbec3b1-1b0f-417d-9059-8b94a47197ed"
  }],
  "outputs": [{
    "name": "stereo",
    "flows": ["6fbec3b1-1b0f-417d-9059-8b94a47197ed"],
    "label": "Programme stereo",
    "layout": ["L", "R"],
    "preset": "bs775-3_2-to-2_0"
  }]
}
```

```sh
foldz --config /etc/foldz/config.json
```

or with the image, as the uid that owns the domain:

```sh
docker run --rm --user "$(id -u):$(id -g)" --read-only --tmpfs /tmp \
  --cap-add SYS_NICE --cap-add IPC_LOCK \
  -v /dev/shm/mxl/studio:/dev/shm/mxl/studio \
  -v /etc/foldz:/etc/foldz:ro -p 8443:8443 \
  ghcr.io/danielclasen/foldz:<version>
```

SIGINT or SIGTERM stops foldz; it deletes its output flows on the way out
and exits with status 0. A config error, a token or TLS file that cannot
be read included, exits with status 2 before anything starts. A listener
that cannot bind stops the start before any flow is created and exits
with status 1, as does an engine that fails to start or a listener that
fails while serving.

### Real-time setup

foldz runs its audio thread `SCHED_FIFO` (`rt.priority`, default 70) and
locks its memory with `mlockall(MCL_CURRENT | MCL_FUTURE)`. Either needs a
privilege: `CAP_SYS_NICE` or an `RLIMIT_RTPRIO` at least the priority, and
`CAP_IPC_LOCK` or an unlimited `RLIMIT_MEMLOCK`. Without it foldz runs
degraded and says so at start and in `/api/v1/status` (`rt`). The image's
binary carries the file capabilities `cap_sys_nice,cap_ipc_lock=p`: they
take effect when the container is granted both capabilities (with no
`no-new-privileges`), and change nothing otherwise. For a binary outside
the image, either set the same capabilities or raise the limits:

```sh
sudo setcap cap_sys_nice,cap_ipc_lock=p ./foldz
# or
ulimit -r 99 -l unlimited
```

Swap off and `vm.compact_unevictable_allowed=0` are deployment
requirements of the samplez engine; with the latter at 1, memory
compaction moves locked pages, which costs the audio thread page faults.

## Configuration

One JSON file, `foldz --config <file>`. Unknown fields are refused.

| Key | Default | |
|---|---|---|
| `mxl.libraryPath` | `/opt/mxl/lib/libmxl.so` | libmxl, loaded at start |
| `engine.sampleRate` | `48000` | the only rate accepted |
| `engine.blockSize` | `48` | samples per block, 16 to 1024 |
| `engine.readerMarginSamples` | `576` | read delay behind libmxl's clock |
| `engine.maxCatchupBlocks` | `2` | late blocks processed in order before skipping to the newest |
| `engine.pruneAfterBlocks` | `50` | blocks a stalled input stays in the read group |
| `engine.rampSamples` | `480` | input fade in after a gap |
| `rt.enabled` | `true` | `false` skips capabilities, memory locking and `SCHED_FIFO` |
| `rt.priority` | `70` | `SCHED_FIFO` priority, 1 to 99 |
| `rt.rtCpu` | `auto` | `auto`, `none`, `first`, `last` or an index into the allowed CPUs |
| `listeners.api` | `127.0.0.1:8080` | `{bind, port, tls}`: `/api/v1` |
| `listeners.ops` | `0.0.0.0:9090` | `{bind, port, tls}`: `/healthz`, `/readyz`, `/metrics` |
| `listeners.nmos` | `127.0.0.1:8081` | `{bind, port, tls}`: bound, serves nothing in this version |
| `listeners.*.tls` | `{mode: none}` | `mode` `none`, `files` (`certFile`, `keyFile`, `clientCaFile`) or `selfSigned` (`sans`) |
| `auth.mode` | `none` | `none`, `token` or `proxy` |
| `auth.token.file` | | the bearer token, `token` mode |
| `auth.proxy` | | `userHeader`, `groupsHeader`, `groupRoles`, `proxy` mode |
| `api.allowedHosts` | `[]` | `Host` names accepted besides `localhost` and `127.0.0.1` |
| `api.allowedOrigins` | `[]` | WebSocket `Origin`s; foldz serves no WebSocket |
| `nmos` | | `enabled` (only `false`), `seed`, `seedName`, `advertiseHost`, `advertisePort`, `registry.url`, `registry.dnsSdDomain`; checked, not used |
| `domains[]` | | `name`, `path` (absolute); at least one |
| `inputs[]` | | `name`, `channels`, `layout` (one label per channel), `domain` (required with several domains), `flow` (UUID; absent or `null` leaves the input unread) |
| `outputs[]` | | see below |

An output:

| Key | |
|---|---|
| `name` | unique among inputs and outputs |
| `input` | the input read; required with several inputs |
| `domain` | required with several domains |
| `flows` | `[<UUID>]`, the flow written |
| `label` | the flow's label; the output's name when empty |
| `group`, `role` | the flow's `urn:x-nmos:tag:grouphint/v1.0` tag `<group>:<role>`; default the output's name and `Audio`; no `:` |
| `layout` | one label per output channel |
| `preset` | a preset name, `select` or `shuffle`; or `matrix` instead |
| `source` | with `shuffle`: per output channel the 0-based input channel |
| `matrix` | rows per output channel, columns per input channel |
| `lfe` | `drop` or `{"include": {"gainDb": <dB>}}` |
| `gainDb` | default `0` |

An output reading an input without a flow is written silent. Flow ids are
lowercase UUIDs, and no flow is both read and written.

## API

| Listener | Path | |
|---|---|---|
| api | `GET /api/v1/downmix` | every input and output with the gain matrix applied, `gainDb` included, and the preset's citation |
| api | `GET /api/v1/status` | RT setup steps, input and output state, engine counters, TLS certificates in use |
| api | `POST /api/v1/login` | `token` mode: the token for a session cookie |
| ops | `GET /healthz` | 200 while the audio thread processed a block within the last second |
| ops | `GET /readyz` | 200 while the engine runs and the listeners serve |
| ops | `GET /metrics` | Prometheus text: `samplez_rt_heartbeat_blocks_total`, `samplez_deadline_miss_total`, `samplez_overrun_skipped_samples_total`, `samplez_input_*_total`, `samplez_output_*_total`, `samplez_clock_steps_total` and others |

`openapi.json` is the OpenAPI 3.1 document of `/api/v1` in `token` mode;
a test fails when it differs from the code.

## Building

CI builds and tests inside the samplez builder image, pinned by digest in
`.github/workflows/ci.yml`:

```sh
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD:/w" \
  -v "$PWD/.cargo-registry:/usr/local/cargo/registry" \
  -v "$PWD/.cargo-git:/usr/local/cargo/git" -w /w \
  ghcr.io/danielclasen/samplez-builder@sha256:92ece4452be98fff177b49fb201c8c693130c08845c42725dd5b1173d4863fba \
  cargo test --workspace --locked
```

The end-to-end test runs the binary against libmxl flows and is run on
its own, with the RT limits raised:

```sh
docker run --rm --user "$(id -u):$(id -g)" --ulimit rtprio=99 --ulimit memlock=-1 \
  -e SAMPLEZ_REQUIRE_RT=1 ... cargo test --locked --test e2e
```

The image:

```sh
docker build -t foldz:local .
docker/smoke.sh foldz:local
```

## Licence

Apache-2.0. The image carries the licence texts of what it ships:
`/usr/share/licenses/foldz` (foldz and the Rust crates in the binary) and
`/opt/mxl/share/licenses` (libmxl and its dependencies).
