//! Opening the "How To Use" guide. The site is no longer embedded in the
//! exe - it is downloaded (see `guide.rs` and its tests) - so what is left
//! here is the opening itself: the sentence a failure shows, and the copy a
//! development build opens straight from the repository.

use v2_lib::guide::{DAMAGED, DOWNLOAD_FAILED, NOT_DOWNLOADED, NOT_PUBLISHED};
use std::collections::BTreeSet;
use v2_lib::help::{guide_address, guide_opening, launcher_html, GuideOpening, DEV_INDEX, OPEN_ERROR};
use v2_lib::modules::Module;

fn mods(ms: &[Module]) -> BTreeSet<Module> {
    ms.iter().copied().collect()
}

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

/// The guide is told which modules are on through its address:
/// `?modules=` with their snake_case names in tree order while any is,
/// nothing while none is, and the guide hides every module's pages it is
/// not told about.
#[test]
fn the_guide_address_names_the_modules_that_are_on() {
    let index = std::path::Path::new(DEV_INDEX);
    let all: BTreeSet<Module> = Module::ALL.into_iter().collect();
    let on = guide_address(index, &all).expect("a file URL");
    let off = guide_address(index, &mods(&[])).expect("a file URL");
    assert!(
        on.starts_with("file:///")
            && on.contains("/help/index.html?modules=db_access,writing_style,auto_run,api_templates,playwright,crystal_reports"),
        "{on}"
    );
    let some = guide_address(index, &mods(&[Module::AutoRun, Module::DbAccess])).expect("a file URL");
    assert!(some.contains("/help/index.html?modules=db_access,auto_run"), "{some}");
    // Transitional for one release: while Auto Run is on the query also
    // carries the old flag, so a guide downloaded under an earlier version
    // still shows its Auto Run pages. Nothing else carries it.
    assert!(
        on.ends_with("?modules=db_access,writing_style,auto_run,api_templates,playwright,crystal_reports&advanced=1"),
        "{on}"
    );
    assert!(some.ends_with("?modules=db_access,auto_run&advanced=1"), "{some}");
    let no_auto_run = guide_address(index, &mods(&[Module::DbAccess, Module::WritingStyle])).expect("a file URL");
    assert!(no_auto_run.ends_with("/help/index.html?modules=db_access,writing_style"), "{no_auto_run}");
    assert!(!no_auto_run.contains("advanced"), "{no_auto_run}");
    assert!(off.starts_with("file:///") && off.ends_with("/help/index.html"), "{off}");
    assert!(!off.contains('?'), "{off}");
    // A path with spaces (this repository's own) is encoded, not cut short.
    assert!(!on.contains(' '), "{on}");
}

/// The launcher sends the browser on to that address, by script and by a
/// meta refresh, and carries nothing else.
#[test]
fn the_launcher_goes_on_to_the_guide() {
    let address = "file:///C:/a%20b/help/index.html?modules=db_access,auto_run";
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

/// With no module on, the index opens as it is: no launcher is written.
/// With any on, the launcher names them.
#[test]
fn no_module_opens_the_index_itself_and_any_goes_through_the_launcher() {
    let index = std::path::Path::new(DEV_INDEX);
    assert_eq!(guide_opening(index, &mods(&[])), GuideOpening::Index);
    match guide_opening(index, &mods(&[Module::WritingStyle])) {
        GuideOpening::Launcher(page) => assert!(page.contains("index.html?modules=writing_style"), "{page}"),
        other => panic!("{other:?}"),
    }
}
