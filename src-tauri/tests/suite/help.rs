//! Opening the "How To Use" guide. The site is no longer embedded in the
//! exe - it is downloaded (see `guide.rs` and its tests) - so what is left
//! here is the opening itself: the sentence a failure shows, and the copy a
//! development build opens straight from the repository.

use v2_lib::guide::{DAMAGED, DOWNLOAD_FAILED, NOT_DOWNLOADED, NOT_PUBLISHED};
use v2_lib::help::{DEV_INDEX, OPEN_ERROR};

/// No sentence the guide's buttons can show names a URL or a path.
#[test]
fn the_sentences_name_no_url_or_path() {
    for s in [OPEN_ERROR, NOT_DOWNLOADED, DOWNLOAD_FAILED, DAMAGED, NOT_PUBLISHED] {
        let lower = s.to_lowercase();
        assert!(!lower.contains("http") && !lower.contains("github"), "{s}");
        assert!(!s.contains(['/', '\\', ':']), "{s}");
    }
}

/// A development build opens the site `npm run docs:build` writes, straight
/// from the repository, so a guide change needs no download to be seen.
#[test]
fn a_development_build_opens_the_repository_copy() {
    let expected = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("help").join("index.html");
    assert_eq!(std::path::Path::new(DEV_INDEX), expected.as_path());
    assert!(expected.is_file(), "the built site is missing - run npm run docs:build");
}

/// The site no longer rides inside the exe.
#[test]
fn the_site_is_not_embedded() {
    let cargo = include_str!("../../Cargo.toml");
    assert!(!cargo.contains("include_dir"), "the include_dir dependency is back");
    let help = include_str!("../../src/help.rs");
    assert!(!help.contains("include_dir!"), "help.rs embeds the site again");
}
