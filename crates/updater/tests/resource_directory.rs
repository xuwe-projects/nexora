use std::path::{Path, PathBuf};

#[path = "../src/release/resource_directory.rs"]
mod resource_directory;

use resource_directory::ExecutableLayout;

#[test]
fn macos_bundle_executable_uses_contents_resources() {
    let executable = Path::new("/Applications/Nexora.app/Contents/MacOS/desktop");

    assert_eq!(
        resource_directory::from_executable(executable, ExecutableLayout::MacOs).unwrap(),
        PathBuf::from("/Applications/Nexora.app/Contents/Resources")
    );
}

#[test]
fn macos_development_executable_keeps_its_own_directory() {
    let executable = Path::new("/workspace/target/debug/desktop");

    assert_eq!(
        resource_directory::from_executable(executable, ExecutableLayout::MacOs).unwrap(),
        PathBuf::from("/workspace/target/debug")
    );
}

#[test]
fn windows_executable_uses_its_install_directory() {
    let executable = Path::new("C:/Users/tester/AppData/Local/Programs/Nexora/desktop.exe");

    assert_eq!(
        resource_directory::from_executable(executable, ExecutableLayout::ExecutableDirectory)
            .unwrap(),
        executable.parent().unwrap()
    );
}
