use std::process::Command;

#[test]
fn version_flags_are_supported() {
    for flag in ["--version", "-v"] {
        let output = Command::new(env!("CARGO_BIN_EXE_xuwecli"))
            .arg(flag)
            .output()
            .unwrap();

        assert!(output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            format!("xuwecli {}\n", env!("CARGO_PKG_VERSION"))
        );
    }
}
