# Using foldz

How foldz behaves and how it is configured and run, one section per
feature. How it is built on samplez is in
[ARCHITECTURE.md](ARCHITECTURE.md), building and testing in
[BUILDING.md](BUILDING.md), and the Helm chart's values in
[charts/foldz](../charts/foldz/README.md).

## Configuration

One JSON file, `foldz --config <file>`. Unknown fields are refused.

| Key | Default | |
|---|---|---|
| `mxl.libraryPath` | `/opt/mxl/lib/libmxl.so` | libmxl, loaded at start |
| `engine.sampleRate` | `48000` | the only rate accepted |
| `engine.blockSize` | `48` | samples per block, 16 to 1024 |
| `engine.readerMarginSamples` | `576` | read delay behind libmxl's clock |
| `engine.maxCatchupBlocks` | `2` | overdue blocks processed in order after a late wake |
| `engine.pruneAfterBlocks` | `50` | late blocks of a frozen producer head before the input is pruned |
| `engine.rampSamples` | `480` | length of every gain ramp and input fade |
| `rt.enabled` | `true` | `false` skips capabilities, memory locking and `SCHED_FIFO` |
| `rt.priority` | `70` | `SCHED_FIFO` priority, 1 to 99 |
| `rt.rtCpu` | `auto` | `auto`, `none`, `first`, `last` or an index into the allowed CPUs |
| `listeners.api` | `127.0.0.1:8080` | `{bind, port, tls}`: `/api/v1` |
| `listeners.ops` | `0.0.0.0:9090` | `{bind, port, tls}`: `/healthz`, `/readyz`, `/metrics` |
| `listeners.nmos` | `127.0.0.1:8081` | `{bind, port, tls}`: `/x-nmos/` with NMOS enabled, bound either way; TLS refused with NMOS enabled |
| `listeners.machine` | | refused |
| `listeners.*.tls` | `{mode: none}` | [HTTPS](#https) |
| `auth.mode` | `none` | `none`, `token` or `proxy` ([Authentication](#authentication)); `oidc` is refused |
| `auth.token.file` | | the bearer token, `token` mode |
| `auth.proxy` | | `userHeader`, `groupsHeader`, `groupRoles`, `proxy` mode |
| `auth.machineToken` | | refused |
| `auth.oidc` | | refused |
| `api.allowedHosts` | `[]` | `Host` names accepted besides `localhost` and `127.0.0.1` |
| `api.allowedOrigins` | `[]` | WebSocket `Origin`s; foldz serves no WebSocket |
| `nmos.enabled` | `false` | registers the inputs and outputs and takes IS-05 connections ([NMOS](#nmos)) |
| `nmos.seed`, `nmos.seedName` | | a UUID or a name the resource ids derive from; exactly one is required with NMOS enabled, and the same one gives the same ids after a restart |
| `nmos.advertiseHost` | | host or address controllers reach the NMOS listener at; required with NMOS enabled |
| `nmos.advertisePort` | `0` | `0` advertises `listeners.nmos.port` |
| `nmos.registry.url` | | a Registration API, `http://host[:port]` with NMOS enabled; set, no DNS-SD browsing |
| `nmos.registry.dnsSdDomain` | | the unicast DNS-SD domain browsed; empty browses the resolver's search domains, then multicast DNS-SD |
| `nmos.heartbeatS` | `5` | seconds between heartbeats, also each registry request's timeout; 1 to 60 |
| `nmos.asset.manufacturer` | `danielclasen` | the BCP-002-02 manufacturer tag |
| `domains[]` | | `name`, `path` (absolute, normalised, unique), `id` (the domain's MXL UUID, for NMOS); at least one |
| `inputs[]` | | `name`, `channels`, `layout` (one label per channel), `domain` (required with several domains), `flow` (UUID; absent or `null` starts the input unconnected); at least one |
| `outputs[]` | | below; at least one |

The `engine` and `rt` keys are passed to the samplez engine, at the
release `Cargo.toml` pins; the table above is what this build accepts, and
samplez v0.8.0 describes the engine's configuration in
[samplez-hop](https://github.com/danielclasen/samplez/blob/v0.8.0/README.md#samplez-hop).

An output:

| Key | |
|---|---|
| `name` | unique among inputs and outputs |
| `input` | the input read; required with several inputs |
| `domain` | required with several domains |
| `flows` | `[<UUID>]`, the flow written; more than one entry is refused |
| `mergedFlow` | refused |
| `label` | the flow's label; the output's name when empty |
| `group`, `role` | the flow's `urn:x-nmos:tag:grouphint/v1.0` tag `<group>:<role>`; default the output's name and `Audio`; no `:` |
| `layout` | one label per output channel ([Channel labels](#channel-labels)) |
| `preset` | a preset name, `select` or `shuffle`; or `matrix` instead |
| `source` | with `shuffle`: per output channel the 0-based input channel |
| `matrix` | rows per output channel, columns per input channel |
| `lfe` | `"drop"` or `{"include": {"gainDb": <dB>}}` |
| `gainDb` | default `0` |

An output reading an unconnected input is written silent. Flow ids are
lowercase UUIDs; the nil UUID is reserved. With NMOS enabled, flow and
domain ids are MXL UUIDs: a version digit of 1 to f and the variant 8 to
b. An id names a flow within its domain: no flow is read by two inputs or
both read and written in one domain, while the same id read in another
domain is another flow. Every written id is unique across all domains.

## Presets and routings

### Channel labels

Channel labels are the IS-04 v1.3 `source_audio.json` channel symbols
(`L`, `R`, `C`, `LFE`, `Ls`, `Rs`, `Lss`, `Rss`, `Lrs`, `Rrs`, `Lc`, `Rc`,
`Cs`, `HI`, `VIN`, `M1`, `M2`, `Lt`, `Rt`, `Lst`, `Rst`, `S`, `NSC000` to
`NSC128`, `U01` to `U64`), plus `-` for an unused channel. A layout has 1
to 64 channels and no label twice except `-`.

### Presets

A preset reads its input channels by label, wherever they sit in the input
layout, and the output layout must be exactly the channels it produces.
Coefficients are transcribed from the cited tables and applied without
headroom scaling; `gainDb` per output scales the whole matrix.

| Preset | Reads | Output layout | Source |
|---|---|---|---|
| `bs775-3_2-to-1_0` | `L R C Ls Rs` | `C` | ITU-R BS.775-4 (12/2022), Annex 4, Table 2 |
| `bs775-3_2-to-2_0` | `L R C Ls Rs` | `L R` | ITU-R BS.775-4 (12/2022), Annex 4, Table 2 |
| `bs775-3_2-to-3_0` | `L R C Ls Rs` | `L R C` | ITU-R BS.775-4 (12/2022), Annex 4, Table 2 |
| `bs775-3_2-to-2_1` | `L R C Ls Rs` | `L R S` | ITU-R BS.775-4 (12/2022), Annex 4, Table 2 |
| `bs775-3_2-to-3_1` | `L R C Ls Rs` | `L R C S` | ITU-R BS.775-4 (12/2022), Annex 4, Table 2 |
| `bs775-3_2-to-2_2` | `L R C Ls Rs` | `L R Ls Rs` | ITU-R BS.775-4 (12/2022), Annex 4, Table 2 |
| `a52-loro-clev<c>-slev<s>`, `c` one of `0.707`, `0.595`, `0.500`, `s` one of `0.707`, `0.500`, `0` | `L R C Ls Rs`, `L R C` with `slev0` | `L R` | ATSC A/52:2018, clause 7.8.2, Tables 5.9 and 5.10 |
| `a52-ltrt` | `L R C Ls Rs` | `Lt Rt` | ATSC A/52:2018, clause 7.8.2 |
| `ac4-7_1-to-5_1` | `L R C LFE Lss Rss Lrs Rrs` | `L R C LFE Ls Rs` | ETSI TS 103 190-1 V1.4.1, clause 6.2.17.5, Table 219, row 3/4/0.x; AC-4 side surrounds read `Lss`/`Rss` and back surrounds `Lrs`/`Rrs` |
| `ac4-bs775-7_1-to-2_0` | `L R C Lss Rss Lrs Rrs` | `L R` | `ac4-7_1-to-5_1` followed by `bs775-3_2-to-2_0`, the product of the two matrices |

### Routings and matrices

`select` copies input channels by label, `shuffle` by 0-based index from
`source`, and `matrix` takes an explicit gain matrix with one row per
output channel.

### LFE

A preset without an `LFE` output leaves the input's LFE out (`lfe:
"drop"`, the default). `lfe: {"include": {"gainDb": <dB>}}` adds it to
every output channel of a 1/0 or 2/0 preset at that gain; the output's
`gainDb` then scales it like every other gain, so LFE arrives at the sum
of the two. `lfe` is refused on `ac4-7_1-to-5_1`, `select`, `shuffle` and
`matrix`.

## API

| Listener | Path | |
|---|---|---|
| api | `GET /api/v1/downmix` | every input and output with the gain matrix applied, `gainDb` included, and the preset's citation |
| api | `GET /api/v1/status` | RT setup steps, input and output state and the flow each input reads, engine counters, TLS certificates in use, each NMOS port's connection and last activation; 503 once the engine has stopped |
| api | `POST /api/v1/login` | `token` mode: the token for a session cookie |
| ops | `GET /healthz` | 200 while the audio thread processed a block within the last second |
| ops | `GET /readyz` | 200 while the engine runs and the API listener serves |
| ops | `GET /metrics` | Prometheus text, below |
| nmos | `/x-nmos/node/v1.3/`, `/x-nmos/connection/v1.2/` | with NMOS enabled: the IS-04 Node API and the IS-05 Connection API |

`/metrics` carries the engine counters of `/api/v1/status`
(`samplez_rt_heartbeat_blocks_total`, `samplez_deadline_miss_total`,
`samplez_overrun_skipped_samples_total` and others), the maxima
`samplez_wake_jitter_max_nanoseconds` and
`samplez_process_time_max_nanoseconds` (counters, since start), per input
once connected `samplez_input_reads_total{input, outcome}` and
`samplez_input_validity_probes_total{input}`, and with NMOS
`samplez_nmos_apply_failures_total{port}` and
`samplez_nmos_activation_late_seconds{port}`.

`openapi.json` is the OpenAPI 3.1 document of `/api/v1` in `token` mode;
a test fails when it differs from the code.

## NMOS

With `nmos.enabled`, foldz is one IS-04 v1.3 Node with one Device
(BCP-002-02 function `Downmix`). Each output is a Source, Flow and Sender
and each input a Receiver, all with transport `urn:x-nmos:transport:mxl`
(AMWA BCP-007-03), grouped by BCP-002-01 grouphints: a Sender's is the
output's `group` with the role `Audio out`, so it names the same group as
the MXL flow's grouphint, whose role is the output's `role`; a Receiver's
is `<input name>:Audio in`. No two outputs may share a `group`. Flows are
`audio/float32` at 48 kHz. A Receiver's BCP-004-01 caps take exactly the
input's channel count. The ids are UUIDv5 of the seed and the input's or
output's name.

foldz finds a registry at `nmos.registry.url`, else by unicast DNS-SD, else
by multicast DNS-SD, heartbeats it, and deletes its Node from it on SIGTERM
before its flows go, waiting at most 5 s for the registry's answer. With
no registry found it is not registered, and it does not advertise itself
peer to peer.

Each domain's MXL id is the `id` of its `domain_def.json` when the domain
has one, else `domains[].id`; both present and different, or neither, stop
foldz at start. A port's `mxl_domain_id` takes only its own domain.

IS-05 v1.2, `/single` and `/bulk`:

- A receiver's `mxl_flow_id` is any MXL UUID but a flow one of foldz's
  outputs writes in that domain; a flow present in the domain must match
  the caps, an absent one is accepted and read once it appears.
  `master_enable: true` routes the input to the flow with the engine's
  fade; `false` detaches it, and its outputs read silence.
- A sender's `mxl_flow_id` is the configured flow only. `master_enable:
  false` releases the writer, which deletes the flow; `true` creates it
  again. A flow another writer holds is a failed activation.
- A scheduled activation switches at its time's sample index rounded up to
  a block, at most `engine.blockSize - 1` samples late. foldz applies it
  the engine's buildLead plus 10 ms early, so the engine can stamp that
  index. Each input route or detach takes one generation and the engine
  builds one at a time, so receivers activated together switch one
  buildLead apart.
- An activation the engine refuses, or an output whose flow is not created
  within 5 s, turns the port's `master_enable` false in `/active`, counts
  in `samplez_nmos_apply_failures_total` and shows in `/api/v1/status`;
  the input is detached from the activation's index, or the output
  disabled. A detach the engine refuses in turn is not repeated: the input
  reads on, and the logged failure, also in `/api/v1/status`, names the
  flow it reads at that moment. A route the engine stamps later than asked is
  logged with the sample it switches at.

Not covered:

- IS-05 state is held in memory: a restart starts from the config, and
  scheduled activations still pending are lost.
- A scheduled receiver activation cancelled within buildLead of its time
  is already stamped: the cancelled flow plays from that time for up to a
  buildLead before the route back lands.
- Scheduled activations wait on the monotonic clock from the time they are
  received: a TAI clock step in between moves them by the step.
- The NMOS listener has no authentication and no TLS (IS-10 and
  BCP-003-01 are out of scope), and IS-05 on it re-routes inputs and stops
  outputs. Its bind address and a NetworkPolicy are what protect it. foldz
  warns at start when it is bound to a loopback address and advertised as
  another host.

## Release binary

Each release carries the binary, the licences of the Rust crates in it
and their SHA-256 sums ([Releases](BUILDING.md#releases)):

```sh
gh release download v<version> --repo danielclasen/foldz --pattern 'foldz-*'
sha256sum -c foldz-<version>-SHA256SUMS
install -m 0755 foldz-<version>-x86_64-linux ./foldz
./foldz --version
```

It loads libmxl v1.1.0 from `mxl.libraryPath` at start. The image carries
that build at `/opt/mxl/lib` and the binary at `/usr/local/bin/foldz`:

```sh
id=$(docker create ghcr.io/danielclasen/foldz:<version>)
docker cp -L "$id:/opt/mxl/lib/libmxl.so" ./libmxl.so
docker cp "$id:/opt/mxl/share/licenses" ./mxl-licenses
docker cp "$id:/usr/local/bin/foldz" ./foldz
docker rm "$id"
```

SIGINT or SIGTERM stops foldz; it deletes its output flows on the way out
and exits with status 0. A usage or config error, a token or TLS file that
cannot be read included, exits with status 2 before anything starts. A
listener that cannot bind stops the start before any flow is created and
exits with status 1, as does an engine that fails to start or a listener
that fails while serving.

## HTTPS

Each listener serves plain HTTP unless its `tls.mode` says otherwise:
`none`, `files` (`certFile`, `keyFile`, `clientCaFile`) or `selfSigned`
(`sans`).

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

A listener with TLS has no plain-HTTP counterpart: foldz does not redirect
HTTP to HTTPS.

## Authentication

`auth.mode` is `none`, `token` or `proxy`; `oidc`, which signs browsers
in for a UI foldz does not serve, is refused. In every mode, a request
without a valid bearer must carry a `Host` of `localhost`, `127.0.0.1` or
a name in `api.allowedHosts`. The ops listener (`/healthz`, `/readyz`,
`/metrics`) is not authenticated.

With `token`, every API request needs the bearer token from
`auth.token.file`, read without trailing whitespace and re-read every
10 s:

```sh
umask 077; openssl rand -hex 32 > /etc/foldz/token
curl --cacert cert.pem -H "Authorization: Bearer $(cat /etc/foldz/token)" \
  https://foldz.example:8443/api/v1/downmix
```

foldz reads the token, certificate and key files as the uid it runs as; a
file that uid cannot read stops the start with exit status 2.

For browsers, `POST /api/v1/login` with the JSON body `{"token": "..."}`
and the header `X-Samplez-Request: 1` exchanges the token for a session
cookie valid for 12 hours. The cookie is marked `Secure` when the listener
serves TLS or the first value of `X-Forwarded-Proto` is `https`.

In `proxy` mode the API listener must bind `127.0.0.1`, and foldz takes
the user and groups from the headers of an authenticating proxy in front
(`auth.proxy.userHeader`, default `X-Forwarded-User`;
`auth.proxy.groupsHeader`, default `X-Forwarded-Groups`, comma-separated);
`auth.proxy.groupRoles` maps group names to `viewer` or `operator` and is
required.

## A complete example

`/etc/foldz/config.json`, a 5.1 flow folded to stereo with BS.775-4, over
HTTPS with a bearer token:

```json
{
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

Run it with the binary, `foldz --config /etc/foldz/config.json`, or with
the image, as the uid that owns the domain and with the real-time
capabilities:

```sh
docker run --rm --user "$(id -u):$(id -g)" --read-only --tmpfs /tmp \
  --cap-add SYS_NICE --cap-add IPC_LOCK \
  -v /dev/shm/mxl/studio:/dev/shm/mxl/studio \
  -v /etc/foldz:/etc/foldz:ro -p 8443:8443 \
  ghcr.io/danielclasen/foldz:<version>
```

## Real-time setup

foldz runs its audio thread `SCHED_FIFO` at `rt.priority` and locks its
memory. That needs `CAP_SYS_NICE` or an `RLIMIT_RTPRIO` at least the
priority, and `CAP_IPC_LOCK` or an unlimited `RLIMIT_MEMLOCK`. Without
them foldz runs degraded and says so at start and in `/api/v1/status`
(`rt`). samplez v0.8.0 describes each step in
[Real-time set-up](https://github.com/danielclasen/samplez/blob/v0.8.0/README.md#real-time-set-up) and what the host needs besides
in [Real-time requirements](https://github.com/danielclasen/samplez/blob/v0.8.0/README.md#real-time-requirements).

The image's binary carries the file capabilities
`cap_sys_nice,cap_ipc_lock=p`: they take effect when the container is
granted both capabilities (with no `no-new-privileges`), and change
nothing otherwise. A binary outside the image needs either the same file
capabilities:

```sh
sudo setcap cap_sys_nice,cap_ipc_lock=p ./foldz
```

or the limits raised in the shell that starts it:

```sh
ulimit -r 99 -l unlimited
```
