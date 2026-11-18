# Changelog

## [1.1.1](https://github.com/duruer/frostpane/compare/v1.1.0...v1.1.1) (2026-11-18)


### Bug Fixes

* build a weather's pipelines before the render thread needs them ([a9b532c](https://github.com/duruer/frostpane/commit/a9b532c82187b1a0bd6de8fc9fcadc19d8b1927a))
* stop the update notice from crashing a hidden app ([74bd24b](https://github.com/duruer/frostpane/commit/74bd24b6d73a927a21b60b630dd9886077e671d5))


### Performance Improvements

* cut GPU device memory from ~800 MB to ~220 MB ([a82bbfb](https://github.com/duruer/frostpane/commit/a82bbfb42855f7bc689fba55c11401baf7acb7ff))
* log overlay frame pacing under FROSTPANE_LOG=debug ([657d98b](https://github.com/duruer/frostpane/commit/657d98b8bab90041fa40631b0e03e2d6f7a528ea))
* memory-map the bold UI font files ([4602a13](https://github.com/duruer/frostpane/commit/4602a132969e3413521a442952d3efa681a3b914))
* slow egui passes to 1 Hz while nothing of egui shows ([0c0d722](https://github.com/duruer/frostpane/commit/0c0d7229bda962257ac51fc6da80f5e2a0ee0c68))
* two overlay swap chain buffers, as the spec has ([557ac0e](https://github.com/duruer/frostpane/commit/557ac0e3e96bed3e93133cb43ac0d99ebd87a66b))
* upload each overlay frame once through a staging belt ([4a8af89](https://github.com/duruer/frostpane/commit/4a8af89c325fe757b208eb5687c8646847c50ead))

# [1.1.0](https://github.com/duruer/frostpane/compare/v1.0.0...v1.1.0) (2026-11-18)


### Features

* add Turkish and Kazakh translations ([eb5c7aa](https://github.com/duruer/frostpane/commit/eb5c7aa180b0326b62b3726ccbb7ceac43940b30))

# 1.0.0 (2026-11-18)


### Bug Fixes

* build on 32-bit Windows ([aa0d114](https://github.com/duruer/frostpane/commit/aa0d114e6da36f1ed55d5a740852bec86c54a63a))


### Features

* initial Frostpane release ([c8570be](https://github.com/duruer/frostpane/commit/c8570beda0d01ffb185644366dbeefd7d8a27181))
