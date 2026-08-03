fn main() -> std::process::ExitCode {
    match nexora::desktop::run_sidecar_from_env_args() {
        Ok(true) => std::process::ExitCode::SUCCESS,
        Ok(false) => {
            eprintln!("updater-macos-sidecar: 缺少 --nexora-updater-sidecar apply 参数");
            std::process::ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("updater-macos-sidecar: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
