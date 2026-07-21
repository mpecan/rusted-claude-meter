# Changelog

## [0.1.2](https://github.com/mpecan/rusted-claude-meter/compare/v0.1.1...v0.1.2) (2026-07-21)


### Features

* **build:** add a "lite" variant that compiles out browser cookie import ([af3c57f](https://github.com/mpecan/rusted-claude-meter/commit/af3c57f28eaebf887e4324a084ab95d1f845db20))
* **build:** lite variant that compiles out browser cookie import (EDR-safe) ([#41](https://github.com/mpecan/rusted-claude-meter/issues/41)) ([74aff1b](https://github.com/mpecan/rusted-claude-meter/commit/74aff1b3eaf74367a1d5cf10bb1a9881410a2d43))


### Documentation

* correct runner cost framing (Linux is 1x, not free) ([3940434](https://github.com/mpecan/rusted-claude-meter/commit/394043430e6112a1c5bec8fb0303d7d73b43057f))
* EDR/antivirus false-positive guidance + release-flow fix ([#39](https://github.com/mpecan/rusted-claude-meter/issues/39)) ([6caf94d](https://github.com/mpecan/rusted-claude-meter/commit/6caf94d2ea1d6412c93d2d6877bbb700cb195700))
* note EDR/antivirus false positives and update release trigger ([a2fbb81](https://github.com/mpecan/rusted-claude-meter/commit/a2fbb8157393f28733a237d167d5983e03f492d8))
* publication-ready README (install, usage, screenshots, ClaudeMeter credit) ([#43](https://github.com/mpecan/rusted-claude-meter/issues/43)) ([173258a](https://github.com/mpecan/rusted-claude-meter/commit/173258aa324b3a77d8e377821de72a6ffaef7248))
* publication-ready README with install, usage, and screenshots ([1ba961d](https://github.com/mpecan/rusted-claude-meter/commit/1ba961d10e5cf2c3c40797ba2842afeb7e3031b0))

## [0.1.1](https://github.com/mpecan/rusted-claude-meter/compare/v0.1.0...v0.1.1) (2026-07-21)


### Features

* browser session import from Chromium + Firefox + Safari ([#10](https://github.com/mpecan/rusted-claude-meter/issues/10)) ([262a028](https://github.com/mpecan/rusted-claude-meter/commit/262a028198b5b032941f7d60956651f4fdd3e6d4))
* collapse less-common browser import targets under "More" ([9f84908](https://github.com/mpecan/rusted-claude-meter/commit/9f84908cd82ccaf9cf76c0b5ba8931f502a93d54))
* **core:** pace ratio, projections and PaceSignal ([#16](https://github.com/mpecan/rusted-claude-meter/issues/16)) ([cae90ed](https://github.com/mpecan/rusted-claude-meter/commit/cae90eddf39562b700931af4fc34ed364b5b647a))
* first-run setup wizard ([#11](https://github.com/mpecan/rusted-claude-meter/issues/11)) ([220ee6e](https://github.com/mpecan/rusted-claude-meter/commit/220ee6e148593c5379495c01f8c5f3c90e0a4ff9))
* **icon:** adopt the "Spark Bolt" package icon ([54f52a2](https://github.com/mpecan/rusted-claude-meter/commit/54f52a2b9d504b27dcef6c6a9fad9e836192f230))
* implement all six tray icon styles ([#9](https://github.com/mpecan/rusted-claude-meter/issues/9)) ([10e090f](https://github.com/mpecan/rusted-claude-meter/commit/10e090f8b050a0a7d223ff5a7866c1a6420a3e92))
* implement Claude Meter redesign — switchable popover + settings restyle ([0bc13dd](https://github.com/mpecan/rusted-claude-meter/commit/0bc13ddbbba51a174424b962bfa83e7805f7a9bd))
* launch at login via tauri-plugin-autostart ([#12](https://github.com/mpecan/rusted-claude-meter/issues/12)) ([f4614e0](https://github.com/mpecan/rusted-claude-meter/commit/f4614e04ba23f92ffcc3f76ebc2a5bdddecc27cf))
* live tray updates, Linux menu surface, macOS popover window ([#4](https://github.com/mpecan/rusted-claude-meter/issues/4)) ([b09396a](https://github.com/mpecan/rusted-claude-meter/commit/b09396a85e173bef5db7fdb8caa3ec5b2ef614fa))
* **macos:** native NSPopover for the menu-bar pop-down ([a4ed885](https://github.com/mpecan/rusted-claude-meter/commit/a4ed885720e406b7d9a37b4d18c8b02c45b1822a))
* master switch to enable/disable pace tracking ([#16](https://github.com/mpecan/rusted-claude-meter/issues/16)) ([7e75ab7](https://github.com/mpecan/rusted-claude-meter/commit/7e75ab7c13bebec35c42a484e0b4f33841715dd6))
* move settings into a dedicated window ([d5dbe8d](https://github.com/mpecan/rusted-claude-meter/commit/d5dbe8d199881cce02c8395b7407b22caf16fa5d))
* notification testing, reset-spam fix, and Spark Bolt icon ([62e05ff](https://github.com/mpecan/rusted-claude-meter/commit/62e05ff83bbb9c3336eeaa419fd0100a5f404d00))
* **notifications:** add test button, wire reset toggle, fix reset spam ([5f42b97](https://github.com/mpecan/rusted-claude-meter/commit/5f42b97c63df6533cb98d7f8d71ffa095c901b35))
* pace UI — projections, verdict badge, display-mode + weekly-basis settings ([#16](https://github.com/mpecan/rusted-claude-meter/issues/16)) ([88f5667](https://github.com/mpecan/rusted-claude-meter/commit/88f56672c12e09ad6ffd43261a603a2dd2e3506d))
* pace-based utilization tracking ([#16](https://github.com/mpecan/rusted-claude-meter/issues/16)) ([3123c3e](https://github.com/mpecan/rusted-claude-meter/commit/3123c3edea9ba6ef1981d408b94e0baed4bdaa29))
* pace-first setting + weekly basis, tray pace badge ([#16](https://github.com/mpecan/rusted-claude-meter/issues/16)) ([f2b9725](https://github.com/mpecan/rusted-claude-meter/commit/f2b972578cb5904742c42d5fcc648c33b8f757d1))
* packaging & release CI — signed DMG, AppImage/deb, tag-triggered release ([#14](https://github.com/mpecan/rusted-claude-meter/issues/14)) ([b0c9b20](https://github.com/mpecan/rusted-claude-meter/commit/b0c9b20b5cbcd777cb810f00dc574abd6b485cd7))
* polling scheduler with cache, backoff and wake-from-sleep refresh ([#2](https://github.com/mpecan/rusted-claude-meter/issues/2)) ([abc7f0a](https://github.com/mpecan/rusted-claude-meter/commit/abc7f0ae24431a5fe8f75d94f1726293913a5e22))
* popover usage cards for headline windows and scoped limits ([#5](https://github.com/mpecan/rusted-claude-meter/issues/5)) ([a12009d](https://github.com/mpecan/rusted-claude-meter/commit/a12009d0a39ee9c3d988f37c0d2bf1f2f195d7ec))
* **popover:** size the macOS popover to its content ([4408c0c](https://github.com/mpecan/rusted-claude-meter/commit/4408c0c1fe83fa0321b02abefd892b0948acdf6a))
* **popover:** size the macOS popover to its content ([9271c4b](https://github.com/mpecan/rusted-claude-meter/commit/9271c4bdb5263529738b9322ec0f931cdddb1d25))
* quality hardening — coverage gate, cargo-deny, duplication check ([#15](https://github.com/mpecan/rusted-claude-meter/issues/15)) ([50519ee](https://github.com/mpecan/rusted-claude-meter/commit/50519ee7fb92dd114244e552920760b6fb5177d2))
* **render:** enlarge tray icon artwork to match ClaudeMeter sizing ([eb27a25](https://github.com/mpecan/rusted-claude-meter/commit/eb27a2557914e1f6a5c2c1d4833c87e98cdb2258))
* **render:** flame/snowflake pace badge + pace-first overrides ([#16](https://github.com/mpecan/rusted-claude-meter/issues/16)) ([44db9e1](https://github.com/mpecan/rusted-claude-meter/commit/44db9e1b6ff727e3e06887f0829e9ada090525b8))
* **render:** wide icons with baked percentage text matching ClaudeMeter ([ab3999f](https://github.com/mpecan/rusted-claude-meter/commit/ab3999f8976e5d32c4e156fb347b24ab574b3ef4))
* scaffold Tauri v2 workspace with domain core, API client and quality gates ([473d3a8](https://github.com/mpecan/rusted-claude-meter/commit/473d3a8ef01072ffc8039c9ef49d5a5eb16c02d4))
* secure session-key storage via OS keyring ([#1](https://github.com/mpecan/rusted-claude-meter/issues/1)) ([f0c1b26](https://github.com/mpecan/rusted-claude-meter/commit/f0c1b263cece4a30490efe40e1cd9b24f23b949b))
* settings persistence, per-model visibility toggles, thresholds, intervals ([#6](https://github.com/mpecan/rusted-claude-meter/issues/6)) ([83cec83](https://github.com/mpecan/rusted-claude-meter/commit/83cec83e76d021a1b4e10d258182059b0ebf488d))
* show exact reset time in popover cards (ClaudeMeter PR [#26](https://github.com/mpecan/rusted-claude-meter/issues/26)) ([ac0699e](https://github.com/mpecan/rusted-claude-meter/commit/ac0699e5a8855fb441f57a2bede51939772a0bb7))
* threshold-crossing and reset notifications ([#7](https://github.com/mpecan/rusted-claude-meter/issues/7)) ([acc02c9](https://github.com/mpecan/rusted-claude-meter/commit/acc02c91badf50ef9115ac90486e9e1ed0ba81ed))
* token/cost usage mode with response logging to nail the shape ([514809d](https://github.com/mpecan/rusted-claude-meter/commit/514809d78b1eb01db7a748391f9598144852e2f7))
* token/cost usage mode with response logging to nail the shape ([82ea6d1](https://github.com/mpecan/rusted-claude-meter/commit/82ea6d1f9e7baa217229e6717ae74a3f4a724ff2))
* tray gauge renderer with icon cache and perceptual snapshot tests ([#3](https://github.com/mpecan/rusted-claude-meter/issues/3)) ([b49441f](https://github.com/mpecan/rusted-claude-meter/commit/b49441f0594df397139e192ae14346f2ce5a4132))
* usage.json export for external consumers ([#8](https://github.com/mpecan/rusted-claude-meter/issues/8)) ([e05f417](https://github.com/mpecan/rusted-claude-meter/commit/e05f417e7c0586adfad0eb6870090efb431bac7e))
* use icons for the popover header Refresh/Settings buttons ([b179484](https://github.com/mpecan/rusted-claude-meter/commit/b179484d46a92b6192e73c569aa4181f93175c4b))
* visual icon-style picker with live previews ([6256db0](https://github.com/mpecan/rusted-claude-meter/commit/6256db0c28e250a8264c3179b5c82ceecf7e8229))


### Bug Fixes

* address design-review findings (settings in a dedicated window) ([3ba2032](https://github.com/mpecan/rusted-claude-meter/commit/3ba2032e5fc868a0987c23bc72d65c4668bb8fdc))
* address design-review findings (tray renderer redesign (match ClaudeMeter)) ([5aac62c](https://github.com/mpecan/rusted-claude-meter/commit/5aac62ca321a2e86709b73f9a0cc52d2ac57ee75))
* address pace-tracking review findings (core, [#16](https://github.com/mpecan/rusted-claude-meter/issues/16)) ([d664d7d](https://github.com/mpecan/rusted-claude-meter/commit/d664d7de0e4acd6b2028be56b5329ce47ea5737d))
* address pace-tracking review findings (frontend, [#16](https://github.com/mpecan/rusted-claude-meter/issues/16)) ([c6abfa3](https://github.com/mpecan/rusted-claude-meter/commit/c6abfa3bc649c6df003196657b29f7695899e6b3))
* address pace-tracking review findings (render, [#16](https://github.com/mpecan/rusted-claude-meter/issues/16)) ([7b53975](https://github.com/mpecan/rusted-claude-meter/commit/7b539758b9b646f8412dcef4898769a2083bbd45))
* address pace-tracking review findings (shell, [#16](https://github.com/mpecan/rusted-claude-meter/issues/16)) ([e4635e0](https://github.com/mpecan/rusted-claude-meter/commit/e4635e0bf652a7c687b6cdb5224e0634e0dc46d4))
* address review findings ([#1](https://github.com/mpecan/rusted-claude-meter/issues/1)) ([c568cff](https://github.com/mpecan/rusted-claude-meter/commit/c568cff71926384ffa99a9d74627499388a6a5b5))
* address review findings ([#10](https://github.com/mpecan/rusted-claude-meter/issues/10)) ([d809218](https://github.com/mpecan/rusted-claude-meter/commit/d8092181f1cb0c5d2409f6d47987955bb3ce6ef9))
* address review findings ([#11](https://github.com/mpecan/rusted-claude-meter/issues/11)) ([7d748fd](https://github.com/mpecan/rusted-claude-meter/commit/7d748fdff2147ba047c80af57d675d3abe4e6a16))
* address review findings ([#12](https://github.com/mpecan/rusted-claude-meter/issues/12)) ([fea40f7](https://github.com/mpecan/rusted-claude-meter/commit/fea40f794c6db59317a0ba2f71067f4dc6f0e571))
* address review findings ([#13](https://github.com/mpecan/rusted-claude-meter/issues/13)) ([e8a898f](https://github.com/mpecan/rusted-claude-meter/commit/e8a898f77e717375e27851049a18152962063271))
* address review findings ([#14](https://github.com/mpecan/rusted-claude-meter/issues/14)) ([05bc4c8](https://github.com/mpecan/rusted-claude-meter/commit/05bc4c84d379d4e1fafb10ba17f7f1444cf20829))
* address review findings ([#15](https://github.com/mpecan/rusted-claude-meter/issues/15)) ([d0618c0](https://github.com/mpecan/rusted-claude-meter/commit/d0618c0449d829e59b09e7145099eaab814118b8))
* address review findings ([#2](https://github.com/mpecan/rusted-claude-meter/issues/2)) ([c7ab2ab](https://github.com/mpecan/rusted-claude-meter/commit/c7ab2ab155337ab2e9d378717ff8a6477ba54038))
* address review findings ([#3](https://github.com/mpecan/rusted-claude-meter/issues/3)) ([80b9752](https://github.com/mpecan/rusted-claude-meter/commit/80b9752471d5a01728c1a2a04a8949322e12f3a3))
* address review findings ([#4](https://github.com/mpecan/rusted-claude-meter/issues/4)) ([3db22fb](https://github.com/mpecan/rusted-claude-meter/commit/3db22fb5fd346ab91709a6b39a40b5c6342b9074))
* address review findings ([#5](https://github.com/mpecan/rusted-claude-meter/issues/5)) ([c9379a2](https://github.com/mpecan/rusted-claude-meter/commit/c9379a20efe1c1dc4b44381860f10ff8927c73de))
* address review findings ([#6](https://github.com/mpecan/rusted-claude-meter/issues/6)) ([1a2bcd8](https://github.com/mpecan/rusted-claude-meter/commit/1a2bcd8668ce37aff831b75bf1475b470eda4fb3))
* address review findings ([#7](https://github.com/mpecan/rusted-claude-meter/issues/7)) ([3b49c27](https://github.com/mpecan/rusted-claude-meter/commit/3b49c27de7edd0acfc77c893f9ef2e9e51644b34))
* address review findings ([#8](https://github.com/mpecan/rusted-claude-meter/issues/8)) ([45cd97d](https://github.com/mpecan/rusted-claude-meter/commit/45cd97df275395b28e682bf4b618ae9fe74ec8c8))
* address review findings ([#9](https://github.com/mpecan/rusted-claude-meter/issues/9)) ([7dec2e5](https://github.com/mpecan/rusted-claude-meter/commit/7dec2e53b94d5dcf563b0a95239b98e77e48d417))
* **api:** keep usage windows with a null reset instead of dropping them ([72a7697](https://github.com/mpecan/rusted-claude-meter/commit/72a7697cf6a4529afa63c4ba5dcc27a0df0703b6))
* **ci:** resolve Linux-only clippy errors in the NSPopover window code ([c85da16](https://github.com/mpecan/rusted-claude-meter/commit/c85da169610eaa31da5c283cdfbaa1ef35da1025))
* **docs:** update stale Status section and document the two CI-only checks ([c615f35](https://github.com/mpecan/rusted-claude-meter/commit/c615f352ef5819c63942694941688b3232e7edc5))
* **macos:** fit the NSPopover height to typical content ([a9d50bd](https://github.com/mpecan/rusted-claude-meter/commit/a9d50bd601d99a56c499722c067d270aeb4f5bbb))
* pace-first tray icon always shows the ratio, not just when off-pace ([#16](https://github.com/mpecan/rusted-claude-meter/issues/16)) ([1cb7e4d](https://github.com/mpecan/rusted-claude-meter/commit/1cb7e4dd3ad9cab452480743193b900d20ddfc2b))
* **ui:** add top padding so the popover header clears the arrow ([0643247](https://github.com/mpecan/rusted-claude-meter/commit/0643247da86e9fb0c6ef830ed9d059876de76052))
* **ui:** more top padding above the popover header ([a9a1e2d](https://github.com/mpecan/rusted-claude-meter/commit/a9a1e2d61706e14f8e9ee218518f9164ebf9465b))
* validate pasted session keys everywhere and keep credential I/O off the UI thread ([3c03b67](https://github.com/mpecan/rusted-claude-meter/commit/3c03b6718c44f059837d10d2e6cacfaf04ab993c))


### Documentation

* document the pace-tracking contract in CLAUDE.md ([#16](https://github.com/mpecan/rusted-claude-meter/issues/16)) ([07c85bc](https://github.com/mpecan/rusted-claude-meter/commit/07c85bc50dc17a3f26aa83d08a43ccc260a0e13a))
* refresh README/CLAUDE.md for NSPopover + redesign; expand License ([5199ba2](https://github.com/mpecan/rusted-claude-meter/commit/5199ba29431bac421905375a9b6a50b957454bdd))


### Code Refactoring

* **render:** share one SVG document envelope across all six icon styles ([949e431](https://github.com/mpecan/rusted-claude-meter/commit/949e4317eda5d60eeb4ad83f06bb30bd1adde764))
