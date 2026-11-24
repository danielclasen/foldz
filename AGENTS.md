# Contributor notes (agents and humans)

Rules for working in this repository. Read them before opening a PR or
running an automated assistant against this tree. `README.md` is the
overview, `docs/USAGE.md` describes how foldz behaves and is configured,
`docs/ARCHITECTURE.md` how it is built on samplez, `docs/BUILDING.md` how
it is built, tested and released, and `charts/foldz/README.md` the chart
and its values; this file holds the rules for changing it. A change to
behaviour updates `docs/USAGE.md`, a change to how the parts fit together
`docs/ARCHITECTURE.md`, a change to the build, tests, CI or releases
`docs/BUILDING.md`, and a change to the chart its README, in the same PR.
The docs win on behaviour; each fact lives in one of them, and the others
link to it.

## Use a git worktree

Work in a worktree of its own, e.g.
`git worktree add .claude/worktrees/<topic> -b <topic> origin/main`, so
parallel sessions on one clone do not share an index.

## Architecture

- `src/`: the binary, in Rust.
  - `config.rs`: the start-up configuration's types; `config/check.rs`
    checks it and builds the engine's configuration from it.
  - `layout.rs`: names the channels of a flow.
  - `presets.rs`: the downmix presets and their standards' tables;
    `presets/fold.rs` turns a preset, a routing or an explicit matrix
    into an output's gain matrix.
  - `api.rs`: the read-only HTTP API (`openapi.json`).
  - `metrics.rs`: passes the engine's counters on.
  - `nmos.rs`: IS-04 registration and IS-05 connections over MXL.
  - `state.rs`: a replicated state of one replica that takes no commands
    and stays at revision 0.
- `tests/`: `presets.rs` and `transparency.rs` check the matrices;
  `config.rs` the configuration's checks; `api.rs` the HTTP API against
  `openapi.json`; `nmos.rs` the NMOS node model, domain ids and flow
  definitions;
  `e2e.rs` runs the binary on real libmxl flows; `tests/nmos` runs the
  AMWA conformance suites against the image and gates on their results.
- `charts/foldz`: the Helm chart, with `scripts/chart-test.sh` and
  `scripts/chart-smoke.sh`.
- `docker/`: the image's smoke test.

The audio engine and NMOS stack are
[samplez](https://github.com/danielclasen/samplez), taken only at a
release tag (`tag = "vX.Y.Z"` in `Cargo.toml`), every samplez crate at the
same one. A samplez bump changes the `Cargo.toml` tags, the samplez
entries in `Cargo.lock` and the samplez links in `docs/` that name the
tag, and nothing else, apart from adapting to its API. Links to samplez
behaviour name the pinned tag; only architecture links point at `main`.

## Invariants

- A preset is a transcription of its published standard, coefficient for
  coefficient. Enforced by: `tests/presets.rs` (`bs775_*` and the A/52
  and TS 103 190 cases) against the standards' tables.
- `select` and `shuffle` at 0 dB pass every input sample through bit for
  bit, signed zeros, NaN payloads and denormals included. Enforced by:
  `tests/transparency.rs`.
- Every output block is exactly one of what the engine defines: the input
  through the gain matrix, the same fading in after a silenced block, or
  silence the engine counted. Enforced by: `tests/e2e.rs`.
- The API is read-only and follows `openapi.json`. Enforced by:
  `committed_openapi_document_matches`.

## Build and test

`docs/BUILDING.md` has the build, test and chart commands, and
`.github/workflows/ci.yml` the exact ones CI runs; run them as CI does.

## Tests

- A test proves the change: it fails without it and passes with it.
  Check that by breaking the code once, before relying on the test.
- Timing is judged in ways a loaded runner cannot break: poll for a
  condition with a deadline, count work, or judge a statistic. Never
  read a fixed number of blocks after a fixed wait.
- Audio assertions are independent of the sample and frame rate.
- No skipped tests and no retries that hide a flake. Fix a flake at its
  cause.

## Working style

- Read the code you change, and its callers and tests, before writing.
- State assumptions, and surface tradeoffs rather than picking silently.
- The minimum code that solves the problem: no speculative options,
  abstractions or error handling for impossible states.
- Fail loudly. Work is not complete while anything was skipped, and tests
  do not pass while any were skipped.

## Text

ASCII only in code, comments, commit messages and PR descriptions: no
typographic dashes, quotes, arrows or ellipsis characters. The vendored
skills under `.agents/skills/` are exempt. State facts,
and leave out what cannot be verified from the source, a run or the
authoritative documentation.

## Commits and pull requests

- Conventional Commits with a scope (`fix(presets): ...`), with `!` or a
  `BREAKING CHANGE:` footer for breaking changes. The subject is at most
  72 characters, in the imperative mood; the body wraps at 72.
- A message says why the change exists, in terms still useful read alone
  years later. Write it declaratively, with no narration of what was
  tried, no checklists and no tool-authored `Co-Authored-By` trailers.
- PRs are squash-merged with the prefilled body. release-please takes one
  changelog entry per squash, from the PR title. A PR carrying several
  release-relevant changes uses a `BEGIN_COMMIT_OVERRIDE` block in its
  body.
- Merging is not releasing: merging the release PR release-please opens
  cuts the release.
- Every PR gets an independent review.

## Graphify

A [Graphify](https://github.com/safishamsi/graphify) knowledge graph can
be built locally with `graphify update .`. It is not committed; the
skill and hook are, under `.agents/skills/`. For codebase questions,
prefer `graphify query "<question>"` over wide searches once
`graphify-out/graph.json` exists.

## When in doubt

Ask the maintainer before changing the API, a preset's coefficients, the
release strategy, or anything hard to reverse.
