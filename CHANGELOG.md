# Changelog

# [1.2.0](https://github.com/duruer/frostpane/compare/v1.1.4...v1.2.0) (2026-11-18)


### Bug Fixes

* **macos:** no idle wake-ups from the loop wrapper, menu bar agent policy ([27db0b1](https://github.com/duruer/frostpane/commit/27db0b16b8cc67364af29ddfbdbef5f24758d35a))
* overlay and sprite hooks for scaled X11 and Wayland sessions ([6e84f3d](https://github.com/duruer/frostpane/commit/6e84f3d75987f244a361ce9d11e71fc1ffea797a))
* softer crystal arms on near snowflakes ([85681bf](https://github.com/duruer/frostpane/commit/85681bf008d17376ab86e3bf76b73a58be426526))
* transparent card windows on macOS and Linux ([6edcc20](https://github.com/duruer/frostpane/commit/6edcc205b6555bce32ff5ebef13bf6c8a3c156ea))
* transparent card windows on Windows ([38b022b](https://github.com/duruer/frostpane/commit/38b022b1cee8c92d4fa94b9d66d24869c67e2e52))


### Features

* Frostpane app icon ([5fce59f](https://github.com/duruer/frostpane/commit/5fce59f37e65f0f3d7fd9f4a23bb431851112040))
* install updates from inside the app ([a3819b3](https://github.com/duruer/frostpane/commit/a3819b312417ddedcac94b50b5d6ebb1228697ea))
* **linux:** add platform-free X11 backend logic ([3e6d5ba](https://github.com/duruer/frostpane/commit/3e6d5ba791d31f5ee4eda69c651a05cce6597a26))
* **linux:** implement the X11 backend ([1a40b49](https://github.com/duruer/frostpane/commit/1a40b497f50039f20199d8b03ff87e77f9a6a499))
* **linux:** pure logic for session services ([7d01e85](https://github.com/duruer/frostpane/commit/7d01e85e711bc74939db7fbedbbfebba0e3704f1))
* **linux:** single instance, D-Bus system events, autostart, wallpaper ([8f7f3d8](https://github.com/duruer/frostpane/commit/8f7f3d8c796403e4bc05c1544336ee104663d10b))
* macOS and Linux support ([e33965c](https://github.com/duruer/frostpane/commit/e33965c5f3d6150ff3df5c3ec8e59b21a4c1977b))
* **macos:** implement the macOS backend ([c0c1ba5](https://github.com/duruer/frostpane/commit/c0c1ba5b12115aa2cbda6895b442dbb9584d268c))
* snowflakes with real shapes and a gentle tumble ([78f3c0a](https://github.com/duruer/frostpane/commit/78f3c0a9fe841837331033a176b4ca4b505832f4))
* **wayland:** add platform-free Wayland logic and KWin script ([d13c837](https://github.com/duruer/frostpane/commit/d13c837ba3965acaa6fbb0b2d69a2a51d1c2fa60))
* **wayland:** layer-shell overlay, sprite surfaces and window sources ([8b20185](https://github.com/duruer/frostpane/commit/8b201859741914125eaacd45c56121b5640ede2f))

## [1.1.4](https://github.com/duruer/frostpane/compare/v1.1.3...v1.1.4) (2026-11-18)


### Bug Fixes

* keep the window size when dragging it between monitors ([db78768](https://github.com/duruer/frostpane/commit/db787687e61f86b0481acf909bb68e5ffc153ffd))

## [1.1.3](https://github.com/duruer/frostpane/compare/v1.1.2...v1.1.3) (2026-11-18)


### Bug Fixes

* **release:** read workspace member names from CRLF manifests ([89d9ccb](https://github.com/duruer/frostpane/commit/89d9ccb4b59d7bc45b6bc870be215f9d24874ea2))
* **release:** repair the member-name match in the version bump script ([5326803](https://github.com/duruer/frostpane/commit/53268035cad8871d1992cea0aedf4f16d74e809f))

## [1.1.2](https://github.com/duruer/frostpane/compare/v1.1.1...v1.1.2) (2026-11-18)


### Bug Fixes

* **release:** bump only workspace crates in Cargo.lock ([2c0943f](https://github.com/duruer/frostpane/commit/2c0943f157752495ad9b233a15941f8f04bbf5f5))

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
