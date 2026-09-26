//! The review page's spec pane: each `specs` entry becomes one rendered
//! document - a file on disk, or a wiki page fetched with the user's token.

use std::path::Path;
use v2_lib::ado::AdoClient;
use v2_lib::spec_pane::{prepare_wiki_markdown, render_all, render_file, wiki_title, SpecDoc, SpecSource};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("tcm-v2-spec-pane-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_relative_entry_resolves_against_the_json_directory_and_a_url_is_a_wiki() {
    let base = Path::new("C:/specs/feature");
    match SpecSource::from_entry("Step13.md", base) {
        SpecSource::File(p) => assert_eq!(p, base.join("Step13.md")),
        other => panic!("{other:?}"),
    }
    match SpecSource::from_entry("C:/elsewhere/A.md", base) {
        SpecSource::File(p) => assert_eq!(p, Path::new("C:/elsewhere/A.md")),
        other => panic!("{other:?}"),
    }
    match SpecSource::from_entry(" https://dev.azure.com/o/p/_wiki/wikis/p.wiki/12/Engine ", base) {
        SpecSource::Wiki(u) => assert_eq!(u, "https://dev.azure.com/o/p/_wiki/wikis/p.wiki/12/Engine"),
        other => panic!("{other:?}"),
    }
    // An http URL that is not a wiki page is still a wiki attempt (it will
    // fail with a clear error), never a file named "https:".
    assert!(matches!(SpecSource::from_entry("https://example.com/x.md", base), SpecSource::Wiki(_)));
}

#[test]
fn a_markdown_file_renders_with_its_first_heading_as_the_title() {
    let p = dir().join("Step13-CalculationEngine.md");
    std::fs::write(&p, "# Calculation Engine\n\n## 5.8 Display Rules\n\nThe following config flags only affect how results are shown.\n").unwrap();
    let doc = render_file(&p);
    assert_eq!(doc.title, "Calculation Engine");
    assert_eq!(doc.kind, "file");
    assert!(doc.error.is_none());
    assert!(doc.html.contains("<h2>5.8 Display Rules</h2>"), "{}", doc.html);
    assert!(doc.source.ends_with("Step13-CalculationEngine.md"));
}

#[test]
fn a_file_without_a_heading_is_titled_by_its_name_and_txt_is_preformatted() {
    let p = dir().join("notes.md");
    std::fs::write(&p, "just a paragraph").unwrap();
    assert_eq!(render_file(&p).title, "notes.md");
    let t = dir().join("rules.txt");
    std::fs::write(&t, "1 < 2 & done").unwrap();
    let doc = render_file(&t);
    assert_eq!(doc.title, "rules.txt");
    assert_eq!(doc.html, "<pre>1 &lt; 2 &amp; done</pre>");
}

#[test]
fn a_missing_or_non_text_file_is_an_error_doc_not_a_failure() {
    let missing = render_file(&dir().join("nope.md"));
    assert_eq!(missing.title, "nope.md");
    assert!(missing.error.as_deref().unwrap_or("").starts_with("Could not read nope.md"), "{:?}", missing.error);
    assert!(missing.html.is_empty());
    let docx = dir().join("spec.docx");
    std::fs::write(&docx, b"PK").unwrap();
    assert_eq!(render_file(&docx).error.as_deref(), Some("Not a text spec"));
}

#[test]
fn wiki_markdown_drops_the_toc_and_turns_attachment_images_into_links() {
    let md = "[[_TOC_]]\n\n# Engine\n\n![diagram](/.attachments/flow.png)\n\n![logo](https://cdn.example.com/logo.png)\n";
    let out = prepare_wiki_markdown(md, "https://dev.azure.com/o/p/_wiki/wikis/p.wiki/12/Engine");
    assert!(!out.contains("_TOC_"));
    assert!(out.contains("[diagram (image)](https://dev.azure.com/o/p/_wiki/wikis/p.wiki/12/Engine)"), "{out}");
    assert!(out.contains("![logo](https://cdn.example.com/logo.png)"), "an absolute image stays: {out}");
    assert_eq!(wiki_title("/Calculation-Engine/Display-Rules"), "Display Rules");
    assert_eq!(wiki_title("Overview"), "Overview");
}

#[test]
fn a_wiki_url_names_its_org_and_project() {
    use v2_lib::ado::endpoints::wiki_url_org_project;
    assert_eq!(
        wiki_url_org_project("https://dev.azure.com/PeoplesHR/My%20Project/_wiki/wikis/My.wiki/12/Engine"),
        Some(("PeoplesHR".to_string(), "My Project".to_string()))
    );
    assert_eq!(wiki_url_org_project("https://dev.azure.com/PeoplesHR/_wiki/wikis/x/1/y"), None);
    assert_eq!(wiki_url_org_project("Step13.md"), None);
}

#[tokio::test]
async fn a_wiki_page_renders_and_a_missing_one_is_an_error_doc() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/wiki/wikis/p.wiki/pages/12"))
        .and(query_param("includeContent", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "path": "/Calculation-Engine",
            "content": "[[_TOC_]]\n\n## 5.8 Display Rules\n\nShown, not stored."
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/wiki/wikis/p.wiki/pages/99"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_url("tok".into(), server.uri());
    let ok = format!("{}/o/p/_wiki/wikis/p.wiki/12/Calculation-Engine", server.uri());
    let gone = format!("{}/o/p/_wiki/wikis/p.wiki/99/Gone", server.uri());
    let docs = render_all(
        &[(ok.clone(), Path::new("C:/x").to_path_buf()), (gone.clone(), Path::new("C:/x").to_path_buf()), (ok.clone(), Path::new("C:/y").to_path_buf())],
        Some(&client),
    )
    .await;
    assert_eq!(docs.len(), 2, "the same URL from two files renders once: {docs:?}");
    assert_eq!(docs[0].kind, "wiki");
    assert_eq!(docs[0].title, "Calculation Engine");
    assert_eq!(docs[0].source, ok);
    assert!(docs[0].html.contains("<h2>5.8 Display Rules</h2>"), "{}", docs[0].html);
    assert!(!docs[0].html.contains("_TOC_"));
    let err = docs[1].error.as_deref().unwrap_or("");
    assert!(err.starts_with("Could not fetch this wiki page"), "{err}");
    assert!(!err.contains("http"), "no URL in a user-facing error: {err}");
    // No client (not signed in): an error doc, not a panic.
    let offline = render_all(&[(ok, Path::new("C:/x").to_path_buf())], None).await;
    assert!(offline[0].error.as_deref().unwrap_or("").contains("signed in"), "{:?}", offline[0].error);
    let _ = SpecDoc::default();
}

/// A shell comment in a code block is not the document's title.
#[test]
fn a_heading_inside_a_fenced_code_block_is_not_the_title() {
    let p = dir().join("Setup.md");
    std::fs::write(&p, "```sh\n# install deps\nnpm i\n```\n\n~~~\n# also code\n~~~\n\n# Real Title\n").unwrap();
    assert_eq!(render_file(&p).title, "Real Title");
    let only_code = dir().join("OnlyCode.md");
    std::fs::write(&only_code, "```\n# not a title\n```\n").unwrap();
    assert_eq!(render_file(&only_code).title, "OnlyCode.md");
}

#[test]
fn spec_entries_pair_each_spec_with_its_files_directory() {
    use v2_lib::import_parser::DraftFile;
    use v2_lib::spec_pane::spec_entries;
    let files = vec![
        DraftFile { path: "C:/w/one/cases.json".into(), label: "cases.json".into(), comment: String::new(), specs: vec!["A.md".into(), "https://dev.azure.com/o/p/_wiki/wikis/w/1/X".into()] },
        DraftFile { path: "C:/w/two/more.json".into(), label: "more.json".into(), comment: String::new(), specs: vec!["B.md".into()] },
        DraftFile { path: "C:/w/none.json".into(), label: "none.json".into(), comment: String::new(), specs: vec![] },
    ];
    let e = spec_entries(&files);
    assert_eq!(e.len(), 3);
    assert_eq!(e[0], ("A.md".to_string(), Path::new("C:/w/one").to_path_buf()));
    assert_eq!(e[1].1, Path::new("C:/w/one").to_path_buf());
    assert_eq!(e[2], ("B.md".to_string(), Path::new("C:/w/two").to_path_buf()));
}
