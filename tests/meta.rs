//! Guards invariants the release workflow depends on.

#[test]
fn version_file_matches_crate_version() {
    let manifest = std::fs::read_to_string("Cargo.toml").expect("Cargo.toml");
    let declared = manifest
        .lines()
        .find_map(|l| l.strip_prefix("version = \""))
        .and_then(|l| l.strip_suffix('"'))
        .expect("version in Cargo.toml");

    let file = std::fs::read_to_string("VERSION").expect("VERSION file");

    assert_eq!(
        declared,
        file.trim(),
        "VERSION and Cargo.toml disagree; the release workflow bumps both"
    );
}

#[test]
fn changelog_has_an_unreleased_section() {
    let changelog = std::fs::read_to_string("CHANGELOG.md").expect("CHANGELOG.md");
    assert!(
        changelog.contains("## [Unreleased]"),
        "the release script inserts new versions after '## [Unreleased]'"
    );
}
