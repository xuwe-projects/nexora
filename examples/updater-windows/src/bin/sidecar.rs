fn main() -> std::process::ExitCode {
    match updater::run_sidecar_from_env_args() {
        Ok(true) => std::process::ExitCode::SUCCESS,
        Ok(false) => {
            eprintln!("updater-windows-sidecar: 缺少 --nexora-updater-sidecar apply 参数");
            std::process::ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("updater-windows-sidecar: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
