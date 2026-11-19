fn main() {
  // SkyLight.framework (used by `dock::cgs` for the private macOS Spaces
  // API — see that module's doc comment) lives under PrivateFrameworks,
  // which isn't on the linker's default framework search path.
  #[cfg(target_os = "macos")]
  println!("cargo:rustc-link-search=framework=/System/Library/PrivateFrameworks");

  // On Windows the app manifest (Common-Controls v6) is embedded by the
  // linker for EVERY target, not by tauri-build. tauri-build links its
  // manifest resource into bins only, so test binaries load comctl32 v5, which
  // lacks TaskDialogIndirect. Any test that links window code (the
  // /cli/invoke dispatch does) then dies with STATUS_ENTRYPOINT_NOT_FOUND
  // before main. windows-app-manifest.xml is tauri-build 2.5.5's default,
  // copied verbatim, so the app's own manifest is unchanged.
  let mut attributes = tauri_build::Attributes::new();
  if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
    && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
  {
    let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
      .join("windows-app-manifest.xml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    // No trustInfo block: the tauri-build manifest never had one.
    println!("cargo:rustc-link-arg=/MANIFESTUAC:NO");
    println!("cargo:rustc-link-arg=/WX");
    attributes = attributes
      .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
  }

  tauri_build::try_build(attributes).expect("failed to run tauri-build")
}
