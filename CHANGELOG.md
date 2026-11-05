# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and the project adheres to [Semantic Versioning](https://semver.org/).
Entries are generated from Conventional Commits when a release is cut.

## [Unreleased]

## [0.7.0] - 2026-11-05

### Added

- **cli:** list init under a Commands section
- **cli:** show the config path in -V and make init's --toml discoverable

### Other

- replace the smoke stub's HttpListener with a TcpListener
- stop the windows smoke step failing on cmg's exit code


## [0.6.2] - 2026-11-05

### Fixed

- **config:** find the config on Windows via %APPDATA%


## [0.6.1] - 2026-11-05

### Fixed

- **wait:** exit 1 when the user cancels
- **wait:** return to the menu when feedback is cancelled
- **wait:** give the feedback prompt real line editing


## [0.6.0] - 2026-11-05

### Added

- **spinner:** show the indicator in every mode
- **wait:** render turns as an agent-style transcript

### Other

- **cli:** mute the deprecation warning label


## [0.5.0] - 2026-11-05

### Added

- **cli:** warn when a deprecated interactive-mode flag is used
- **cli:** rename wait mode to interactive mode and show a spinner

### Other

- add README and the MIT licence it refers to


## [0.4.0] - 2026-11-02

### Added

- **wait:** give the review loop a readable transcript


## [0.3.0] - 2026-11-02

### Added

- **cli:** keep --watch as an alias for --wait
- **wait:** separate the menu from the message with a blank line

### Other

- spell out which commits cut a release
- record the CI failures we hit and their shape


## [0.2.2] - 2026-11-02

### Fixed

- **ci:** check out before downloading release assets


## [0.2.1] - 2026-11-02

### Fixed

- **ci:** do not fail native targets in the configure step
- **ci:** give ring a C compiler and make packaging portable


## [0.2.0] - 2026-11-02

### Added

- add -w review loop with menu and feedback
- generate messages for untracked-only changes
- generate commit messages from staged or unstaged changes

### Fixed

- redraw the watch menu in place on one line
- anchor the watch menu at an absolute position
- stop the watch menu drifting on every redraw

### Other

- release automatically on push to master from VERSION
- build release binaries for linux, termux and more
- remove obsolete test file
- add AGENTS.md and ARCHITECTURE.md


<!-- Entries for the next release are added here automatically. -->
