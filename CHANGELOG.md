# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and the project adheres to [Semantic Versioning](https://semver.org/).
Entries are generated from Conventional Commits when a release is cut.

## [Unreleased]

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
