# Changelog

All notable changes to this project will be documented in this file.

## [1.0.0] - 2026-02-17

### Features

- improve CLI help text with grouped arguments and examples ([#33](https://github.com/egeapak/mergers/pull/33))
- add work item status colors from Azure DevOps API ([#34](https://github.com/egeapak/mergers/pull/34))
- update migrate mode layout to match merge mode with table view and toggleable details ([#36](https://github.com/egeapak/mergers/pull/36))
- use Azure DevOps API state colors throughout application ([#37](https://github.com/egeapak/mergers/pull/37))
- add item counts to table titles in both modes ([#39](https://github.com/egeapak/mergers/pull/39))
- display each work item with individual API colors in PR tables ([#41](https://github.com/egeapak/mergers/pull/41))
- sort PR list by close date (newest first) in merge mode ([#42](https://github.com/egeapak/mergers/pull/42))
- migrate to typed state management with compile-time type safety ([#43](https://github.com/egeapak/mergers/pull/43))
- skip git hooks during cherry-pick with optional --run-hooks flag  ([#46](https://github.com/egeapak/mergers/pull/46))
- add associated Config type to AppMode trait for compile-time type safety  ([#49](https://github.com/egeapak/mergers/pull/49))
- add non-interactive merge mode for AI agents and CI systems  ([#50](https://github.com/egeapak/mergers/pull/50))
- add PR dependency analysis for merge workflows ([#54](https://github.com/egeapak/mergers/pull/54))
- add PR dependency UI integration with roaring bitmap optimization  ([#55](https://github.com/egeapak/mergers/pull/55))
- enhance help output with git hash, colors, and syntax highlighting ([#57](https://github.com/egeapak/mergers/pull/57))
- highlight PR dependencies when navigating selection list ([#66](https://github.com/egeapak/mergers/pull/66))
- add step-by-step wizard UI for repository initialization ([#70](https://github.com/egeapak/mergers/pull/70))
- add work item grouping with highlighting and settings dialog ([#72](https://github.com/egeapak/mergers/pull/72))
- add #[must_use] attributes and tracing-based logging support ([#73](https://github.com/egeapak/mergers/pull/73))
- granular wizard steps with channel-based execution and unified state management ([#76](https://github.com/egeapak/mergers/pull/76))
- migrate data loading to channel-based wizard pattern  ([#82](https://github.com/egeapak/mergers/pull/82))
- add tag-based release notes generation and TUI export ([#81](https://github.com/egeapak/mergers/pull/81))

### Bug Fixes

- prevent false positive PR pattern detection in migration mode ([#35](https://github.com/egeapak/mergers/pull/35))
- adjust PR list layout to fill full height when details pane is open ([#38](https://github.com/egeapak/mergers/pull/38))
- improve abort UI responsiveness during conflict resolution ([#47](https://github.com/egeapak/mergers/pull/47))
- improve status command and add git repo project defaults ([#58](https://github.com/egeapak/mergers/pull/58))
- TUI merge mode now fills full terminal height ([#59](https://github.com/egeapak/mergers/pull/59))
- enable dependency analysis with proper tree display ([#60](https://github.com/egeapak/mergers/pull/60))
- restore 'd' hotkey for details pane toggle and fix graph popup help text ([#61](https://github.com/egeapak/mergers/pull/61))
- ensure proper worktree cleanup during TUI merge abort and setup failures ([#62](https://github.com/egeapak/mergers/pull/62))
- prevent JoinHandle polled after completion panic ([#65](https://github.com/egeapak/mergers/pull/65))
- handle empty commits in cherry-pick continue ([#67](https://github.com/egeapak/mergers/pull/67))
- show correct mode name on cleanup confirmation page ([#68](https://github.com/egeapak/mergers/pull/68))
- improve patch branch detection and error messages in cleanup mode ([#71](https://github.com/egeapak/mergers/pull/71))
- add safe utf-8 string truncation to prevent panics ([#75](https://github.com/egeapak/mergers/pull/75))
- detect existing worktrees and branches in CheckPrerequisites step ([#78](https://github.com/egeapak/mergers/pull/78))
- resolve hanging, --since parameter, and CLI argument issues ([#80](https://github.com/egeapak/mergers/pull/80))
- conditionally run dependency analysis based on local repo configuration ([#83](https://github.com/egeapak/mergers/pull/83))
- restrict workflow permissions and resolve code scanning alerts ([#90](https://github.com/egeapak/mergers/pull/90))
- use standard Rust target triples for release artifacts ([#91](https://github.com/egeapak/mergers/pull/91))

### Refactor

- add BrowserOpener trait for testable browser operations ([#40](https://github.com/egeapak/mergers/pull/40))
- improve state management ergonomics with cleaner trait design ([#44](https://github.com/egeapak/mergers/pull/44))
- improve non-interactive mode test coverage and code quality ([#51](https://github.com/egeapak/mergers/pull/51))
- improve PR Dependencies column display and positioning ([#63](https://github.com/egeapak/mergers/pull/63))

### Documentation

- improve README structure, add codecov badge, and add MIT license ([#79](https://github.com/egeapak/mergers/pull/79))

### Styling

- update PR list highlight colors to lighter green ([#32](https://github.com/egeapak/mergers/pull/32))
- add bold and colored styling to hotkey help text ([#45](https://github.com/egeapak/mergers/pull/45))
- add consistent styled hotkeys across all TUI help bars ([#64](https://github.com/egeapak/mergers/pull/64))

### Testing

- increase snapshot test terminal dimensions to 120x50 ([#69](https://github.com/egeapak/mergers/pull/69))
- comprehensive clap argument parsing test coverage ([#84](https://github.com/egeapak/mergers/pull/84))

### Miscellaneous Tasks

- remove multi-platform build job ([#48](https://github.com/egeapak/mergers/pull/48))
- update dependencies and remove unused code ([#56](https://github.com/egeapak/mergers/pull/56))
- upgrade dependencies to latest versions ([#74](https://github.com/egeapak/mergers/pull/74))
- prepare repository for open-source release ([#85](https://github.com/egeapak/mergers/pull/85))
- bump the cargo group across 1 directory with 3 updates ([#88](https://github.com/egeapak/mergers/pull/88))
- pin all dependencies and upgrade with security fix ([#89](https://github.com/egeapak/mergers/pull/89))

### New Contributors

* @dependabot[bot] made their first contribution in [#88](https://github.com/egeapak/mergers/pull/88)
* @ugurtepecik made their first contribution in [#81](https://github.com/egeapak/mergers/pull/81)

## [0.3.0] - 2025-12-10

### Features

- add scrollbar to PR list view ([#18](https://github.com/egeapak/mergers/pull/18))
- improve code architecture with security, error handling, and performance optimizations ([#23](https://github.com/egeapak/mergers/pull/23))
- allow empty commits in cherry-pick and add skip/abort options ([#25](https://github.com/egeapak/mergers/pull/25))
- add EventSource abstraction for testable run_app loop ([#29](https://github.com/egeapak/mergers/pull/29))

### Bug Fixes

- prevent git from prompting for commit message after conflict resolution ([#20](https://github.com/egeapak/mergers/pull/20))
- handle merge commits in cherry-pick with -m flag ([#22](https://github.com/egeapak/mergers/pull/22))
- improve selection color contrast in PR list ([#24](https://github.com/egeapak/mergers/pull/24))

### Refactor

- improve test isolation and add Serde support ([#21](https://github.com/egeapak/mergers/pull/21))
- improve code quality with static regex, error display, and reduced duplication ([#26](https://github.com/egeapak/mergers/pull/26))
- replace hand-rolled API client with azure_devops_rust_api crate ([#30](https://github.com/egeapak/mergers/pull/30))

### Testing

- add comprehensive coverage for migration data_loading module ([#19](https://github.com/egeapak/mergers/pull/19))
- increase API module test coverage from 70% to 97% ([#28](https://github.com/egeapak/mergers/pull/28))

### Miscellaneous Tasks

- upgrade all dependencies to latest versions ([#27](https://github.com/egeapak/mergers/pull/27))

## [0.2.0] - 2025-11-21

### Features

- add real-time feedback for cherry-pick continue operations ([#11](https://github.com/egeapak/mergers/pull/11))
- allow CLI arguments without explicit subcommand, default to merge mode ([#14](https://github.com/egeapak/mergers/pull/14))
- add cherry-pick reference detection for squash merges ([#15](https://github.com/egeapak/mergers/pull/15))

### Bug Fixes

- increase migration mode bottom bar height to display all content ([#10](https://github.com/egeapak/mergers/pull/10))
- display migration success screen with completion statistics ([#9](https://github.com/egeapak/mergers/pull/9))

### Other Changes

- add github token, remove master push trigger ([#8](https://github.com/egeapak/mergers/pull/8))

### Refactor

- restructure CLI to use subcommand pattern with mode as first argument ([#12](https://github.com/egeapak/mergers/pull/12))
- replace pin emoji with checkmark in migration mode manual overrides ([#13](https://github.com/egeapak/mergers/pull/13))

## [0.1.3] - 2025-10-06

### Other Changes

- prepare a release & changelog workflow w/ git-cliff ([#6](https://github.com/egeapak/mergers/pull/6))

### Testing

- Implement snapshot testing ([#2](https://github.com/egeapak/mergers/pull/2))
- add more tests to html-parser ([#3](https://github.com/egeapak/mergers/pull/3))
- add snapshot tests for error page ([#4](https://github.com/egeapak/mergers/pull/4))
- add snapshot tests for remaining ui pages ([#5](https://github.com/egeapak/mergers/pull/5))

## [0.1.2] - 2025-09-22

### Other Changes

- Add README.md for the Azure DevOps merge tool ([#1](https://github.com/egeapak/mergers/pull/1))

### New Contributors

* @egeapak made their first contribution in [#](https://github.com/egeapak/mergers/pull/)
* @google-labs-jules[bot] made their first contribution in [#](https://github.com/egeapak/mergers/pull/)

<!-- generated by git-cliff -->
