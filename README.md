# foldz

Headless broadcast audio downmix and channel matrix over MXL, built on the
[samplez](https://github.com/danielclasen/samplez) engine, written in Rust.

The crate holds the channel layouts and the downmix presets.

## Presets

Coefficients are transcribed from the cited tables and applied without
headroom scaling; `gainDb` per output scales the whole matrix.

| Preset | Source |
|---|---|
| `bs775-3_2-to-1_0`, `-2_0`, `-3_0`, `-2_1`, `-3_1`, `-2_2` | ITU-R BS.775-4 (12/2022), Annex 4, Table 2 |
| `a52-loro-clev<c>-slev<s>`, `c` one of `0.707`, `0.595`, `0.500`, `s` one of `0.707`, `0.500`, `0` | ATSC A/52:2018, clause 7.8.2, Tables 5.9 and 5.10 |
| `a52-ltrt` | ATSC A/52:2018, clause 7.8.2 |
| `ac4-7_1-to-5_1` | ETSI TS 103 190-1 V1.4.1, clause 6.2.17.5, Table 219, row 3/4/0.x |
| `ac4-bs775-7_1-to-2_0` | `ac4-7_1-to-5_1` followed by `bs775-3_2-to-2_0`, the product of the two matrices |

`select` copies input channels by label, `shuffle` by 0-based index from
`source`, and `matrix` takes an explicit gain matrix with one row per output
channel. `lfe: include` adds the input's LFE at `gainDb` to 1/0 and 2/0
outputs; LFE is dropped otherwise.

## Building

CI builds and tests inside the samplez builder image, pinned by digest in
`.github/workflows/ci.yml`:

```sh
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD:/w" \
  -v "$PWD/.cargo-registry:/usr/local/cargo/registry" -w /w \
  ghcr.io/danielclasen/samplez-builder@sha256:92ece4452be98fff177b49fb201c8c693130c08845c42725dd5b1173d4863fba \
  cargo test --workspace --locked
```

## Licence

Apache-2.0.
