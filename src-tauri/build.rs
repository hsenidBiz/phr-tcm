fn main() {
    // Test binaries import comctl32 v6 functions (TaskDialogIndirect via tauri
    // deps) but only the main binary gets tauri-build's manifest. Without a
    // Common-Controls v6 manifest the loader binds System32's legacy comctl32
    // 5.82 and every test exe dies at startup with STATUS_ENTRYPOINT_NOT_FOUND.
    // rustc-link-arg-tests scopes these flags to test targets only, so they
    // never collide with tauri-build's embedded manifest on the app binary.
    #[cfg(windows)]
    {
        println!("cargo::rustc-link-arg-tests=/MANIFEST:EMBED");
        println!(
            "cargo::rustc-link-arg-tests=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' publicKeyToken='6595b64144ccf1df' language='*' processorArchitecture='*'"
        );
    }
    tauri_build::build()
}
