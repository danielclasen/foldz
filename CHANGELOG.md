# Changelog

## [0.3.1](https://github.com/danielclasen/foldz/compare/v0.3.0...v0.3.1) (2026-11-26)


### Bug Fixes

* **deps:** move samplez to v0.28.0 ([#21](https://github.com/danielclasen/foldz/issues/21)) ([ac1a573](https://github.com/danielclasen/foldz/commit/ac1a573a7d425723722873cf8f2def1dd3ab8428))

## [0.3.0](https://github.com/danielclasen/foldz/compare/v0.2.1...v0.3.0) (2026-11-24)


### ⚠ BREAKING CHANGES

* **chart:** nmos.enabled with nmos.service.type NodePort or LoadBalancer now fails the render unless networkPolicy.enabled is true and networkPolicy.nmosFrom lists the peers allowed to reach the NMOS port.
* **config:** auth.mode oidc and auth.oidc stop the start with exit status 2; use none, token or proxy.

### Bug Fixes

* **chart:** refuse an NMOS Service reachable from outside unless policed ([#17](https://github.com/danielclasen/foldz/issues/17)) ([837ee3f](https://github.com/danielclasen/foldz/commit/837ee3f3236c7633e89685d039102d7b888cc9cf))
* **config:** refuse auth.mode oidc at startup ([#16](https://github.com/danielclasen/foldz/issues/16)) ([8925f11](https://github.com/danielclasen/foldz/commit/8925f116cfaec581c7e0b5e3a893c1ff6625cd41))
* **nmos:** stop repeating a refused detach ([#18](https://github.com/danielclasen/foldz/issues/18)) ([5f3371a](https://github.com/danielclasen/foldz/commit/5f3371a065891c46fdda5064a20b1e857bb90321))

## [0.2.1](https://github.com/danielclasen/foldz/compare/v0.2.0...v0.2.1) (2026-11-23)


### Bug Fixes

* **deps:** take url 2.5.9 for RUSTSEC-2026-0592 ([#12](https://github.com/danielclasen/foldz/issues/12)) ([81643d5](https://github.com/danielclasen/foldz/commit/81643d50b4932e1cf82b0490e609c07b996bb58f))
* **main:** end the process at once when the service thread panics ([f701fc2](https://github.com/danielclasen/foldz/commit/f701fc2c5c860853fd5ce7edc2ce1f08cc1aedce))
* **main:** exit as a failure when the service thread panicked ([f701fc2](https://github.com/danielclasen/foldz/commit/f701fc2c5c860853fd5ce7edc2ce1f08cc1aedce))

## [0.2.0](https://github.com/danielclasen/foldz/compare/v0.1.0...v0.2.0) (2026-11-18)


### ⚠ BREAKING CHANGES

* **nmos:** the nil UUID is refused as a flow id or seed, and GET /api/v1/status reports an unconnected input with every slot present, gains a flow per input and an nmos section.

### Features

* **chart:** render the NMOS settings and accept nmos.enabled ([#9](https://github.com/danielclasen/foldz/issues/9)) ([05dfd70](https://github.com/danielclasen/foldz/commit/05dfd700ee6596b6fbe2a93d7430f70f28a6dfd9))
* **nmos:** register inputs and outputs and take IS-05 connections ([29dfe23](https://github.com/danielclasen/foldz/commit/29dfe23dd577ae27ec5ede87bdf5402c173f1ef2))

## 0.1.0 (2026-11-17)


### Features

* add the foldz binary and container image ([#3](https://github.com/danielclasen/foldz/issues/3)) ([abb0d92](https://github.com/danielclasen/foldz/commit/abb0d92c901b1132f909a48185028b684e6487e7))
* **chart:** add the foldz Helm chart ([#4](https://github.com/danielclasen/foldz/issues/4)) ([cb752af](https://github.com/danielclasen/foldz/commit/cb752afab97427939c32533abb17f696fb4e873d))
* **presets:** add cited downmix presets, layouts and config ([#1](https://github.com/danielclasen/foldz/issues/1)) ([32089fb](https://github.com/danielclasen/foldz/commit/32089fb01eb3b6b52080091031c5c8a987dbf43b))
