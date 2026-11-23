# Changelog

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
