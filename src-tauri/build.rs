fn main() {
    #[cfg(target_os = "windows")]
    {
        embed_resource::compile_for_everything("windows-tests.rc", embed_resource::NONE)
            .manifest_required()
            .expect("failed to compile the Windows application manifest");

        let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
        tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
            .expect("failed to build the Tauri Windows resources");
    }

    #[cfg(not(target_os = "windows"))]
    tauri_build::build();
}
