use std::{
    fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use desktop::process::{
    ApplicationIdentity, CoordinatorEvent, ProcessBootstrap, ProcessBootstrapOptions, bootstrap,
};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "nexora-desktop-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        fs::create_dir_all(&path).expect("应当能创建测试目录");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn development_identity_combines_application_name_and_canonical_executable() {
    let directory = TestDirectory::new("identity");
    let executable = directory.path().join("desktop-app");
    fs::write(&executable, b"test").expect("应当能创建测试可执行文件");

    let first = ApplicationIdentity::for_development("Nexora Studio", &executable)
        .expect("规范路径应当能生成身份");
    let second = ApplicationIdentity::for_development("Nexora Studio", &executable)
        .expect("相同输入应当能再次生成身份");
    let another = ApplicationIdentity::for_development("Another App", &executable)
        .expect("另一应用名应当能生成身份");

    assert_eq!(first, second);
    assert_ne!(first, another);
    assert!(first.as_str().starts_with("nexora-studio-"));
}

#[test]
fn explicit_application_identity_uses_registered_app_id() {
    let identity = ApplicationIdentity::explicit("com.example.imes")
        .expect("注册的 app ID 应当能生成稳定应用身份");

    assert_eq!(identity.as_str(), "com.example.imes");
}

#[test]
fn second_bootstrap_activates_existing_main_process() {
    let directory = TestDirectory::new("single-instance");
    let identity =
        ApplicationIdentity::explicit("com.example.nexora-test").expect("显式应用身份应当合法");
    let main = bootstrap(ProcessBootstrapOptions {
        identity: identity.clone(),
        enabled: true,
        runtime_root: Some(directory.path().to_owned()),
    })
    .expect("首次启动应当成为主进程");
    let ProcessBootstrap::Main(main) = main else {
        panic!("首次启动必须成为主进程");
    };

    let secondary = bootstrap(ProcessBootstrapOptions {
        identity,
        enabled: true,
        runtime_root: Some(directory.path().to_owned()),
    })
    .expect("二次启动应当能联系主进程");
    assert!(matches!(secondary, ProcessBootstrap::SecondaryActivated));

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if matches!(main.try_recv(), Some(CoordinatorEvent::ActivateGroup)) {
            break;
        }
        assert!(Instant::now() < deadline, "主进程应当收到激活事件");
        thread::sleep(Duration::from_millis(10));
    }
}
