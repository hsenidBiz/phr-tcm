//! Opening the "How To Use" guide. The site is no longer embedded in the
//! exe - it is downloaded (see `guide.rs` and its tests) - so what is left
//! here is the opening itself: the sentence a failure shows, and the copy a
//! development build opens straight from the repository.

use v2_lib::guide::{DAMAGED, DOWNLOAD_FAILED, NOT_DOWNLOADED, NOT_PUBLISHED};
use v2_lib::help::{guide_address, guide_opening, launcher_html, GuideOpening, DEV_INDEX, OPEN_ERROR};

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

/// The guide is told whether Enable Advanced Features is on through its
/// address: `?advanced=1` while on, nothing while off, and the guide hides
/// Auto Run and API Templates unless it is there.
#[test]
fn the_guide_address_says_whether_advanced_features_are_on() {
    let index = std::path::Path::new(DEV_INDEX);
    let on = guide_address(index, true).expect("a file URL");
    let off = guide_address(index, false).expect("a file URL");
    assert!(on.starts_with("file:///") && on.ends_with("/help/index.html?advanced=1"), "{on}");
    assert!(off.starts_with("file:///") && off.ends_with("/help/index.html"), "{off}");
    assert!(!off.contains('?'), "{off}");
    // A path with spaces (this repository's own) is encoded, not cut short.
    assert!(!on.contains(' '), "{on}");
}

/// The launcher sends the browser on to that address, by script and by a
/// meta refresh, and carries nothing else.
#[test]
fn the_launcher_goes_on_to_the_guide() {
    let address = "file:///C:/a%20b/help/index.html?advanced=1";
    let page = launcher_html(address);
    assert!(page.contains(&format!("location.replace(\"{address}\")")), "{page}");
    assert!(page.contains(&format!("url={address}")), "{page}");
    let odd = launcher_html("file:///x/index.html?a=1&b=\"2\"");
    assert!(odd.contains("url=file:///x/index.html?a=1&amp;b=&quot;2&quot;"), "{odd}");
}

/// Every way the app opens the guide goes through the launcher.
#[test]
fn every_opening_tells_the_guide() {
    for src in [include_str!("../../src/commands/misc.rs"), include_str!("../../src/commands/guide.rs")] {
        assert!(!src.contains("help::open_index("), "an opening skips the launcher");
        assert!(src.contains("help::open_guide("));
    }
}

/// Off, the index opens as it is: no launcher is written. On, the launcher
/// carries the flag.
#[test]
fn off_opens_the_index_itself_and_on_goes_through_the_launcher() {
    let index = std::path::Path::new(DEV_INDEX);
    assert_eq!(guide_opening(index, false), GuideOpening::Index);
    match guide_opening(index, true) {
        GuideOpening::Launcher(page) => assert!(page.contains("index.html?advanced=1"), "{page}"),
        other => panic!("{other:?}"),
    }
}
