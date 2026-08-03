#![cfg(feature = "cli")]

use std::{
    env, fs,
    io::{Read as _, Write as _},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

const SINGLE_MANIFEST_TEMPLATE: &str =
    include_str!("../templates/scaffold/single/Cargo.toml.askama");
const WORKSPACE_MANIFEST_TEMPLATE: &str =
    include_str!("../templates/scaffold/workspace/Cargo.toml.askama");
const DESKTOP_MANIFEST_TEMPLATE: &str =
    include_str!("../templates/scaffold/workspace/apps/desktop/Cargo.toml.askama");
const MAIN_TEMPLATE: &str = include_str!("../templates/scaffold/main.rs");
const FEATURES_TEMPLATE: &str = include_str!("../templates/scaffold/features.rs");
const HOME_FEATURE_TEMPLATE: &str = include_str!("../templates/scaffold/features/home.rs");
const AGENTS_TEMPLATE: &str = include_str!("../templates/scaffold/AGENTS.md");
const GITIGNORE_TEMPLATE: &str = include_str!("../templates/scaffold/gitignore.askama");
const DESKTOP_CONFIG_TEMPLATE: &str =
    include_str!("../templates/scaffold/workspace/apps/desktop/config.rs");
const SERVER_MANIFEST_TEMPLATE: &str =
    include_str!("../templates/scaffold/workspace/apps/server/Cargo.toml.askama");
const SERVER_MAIN_TEMPLATE: &str =
    include_str!("../templates/scaffold/workspace/apps/server/main.rs");
const SERVER_CONFIG_TEMPLATE: &str =
    include_str!("../templates/scaffold/workspace/apps/server/config.rs");
const SERVER_ROUTES_TEMPLATE: &str =
    include_str!("../templates/scaffold/workspace/apps/server/routes.rs");
const EXAMPLE_SERVER_CONFIG_TEMPLATE: &str =
    include_str!("../templates/scaffold/workspace/config/example.server.toml");
const EXAMPLE_DESKTOP_CONFIG_TEMPLATE: &str =
    include_str!("../templates/scaffold/workspace/config/example.desktop.toml");

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new(name: &str) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("系统时钟应晚于 Unix 元年")
            .as_nanos();
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!(
            "nexora-cli-{name}-{}-{timestamp}-{id}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("应能创建隔离的测试目录");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn run(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_nexora"))
            .args(arguments)
            .current_dir(&self.path)
            .output()
            .expect("应能启动 nexora 命令")
    }

    fn run_with_env(&self, arguments: &[&str], envs: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_nexora"));
        command.args(arguments).current_dir(&self.path);
        for (key, value) in envs {
            command.env(key, value);
        }
        command.output().expect("应能启动 nexora 命令")
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[derive(Default)]
struct MockState {
    objects: std::collections::BTreeMap<String, Vec<u8>>,
    puts: Vec<String>,
    replace_latest_after_manifest: Option<(String, Vec<u8>)>,
    fail_put_suffix: Option<String>,
}

struct MockObjectStore {
    address: std::net::SocketAddr,
    state: Arc<Mutex<MockState>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl MockObjectStore {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let state = Arc::new(Mutex::new(MockState::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let server_state = Arc::clone(&state);
        let server_stop = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            while !server_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        let (method, path, body) = read_http_request(&mut stream);
                        let mut state = server_state.lock().unwrap();
                        let (status, response_body) = match method.as_str() {
                            "PUT" => {
                                if state
                                    .fail_put_suffix
                                    .as_ref()
                                    .is_some_and(|suffix| path.ends_with(suffix))
                                {
                                    state.puts.push(path);
                                    ("500 Internal Server Error", Vec::new())
                                } else {
                                    state.objects.insert(path.clone(), body);
                                    state.puts.push(path);
                                    if state
                                        .puts
                                        .last()
                                        .is_some_and(|path| path.contains("/manifests/"))
                                        && let Some((latest_path, latest)) =
                                            state.replace_latest_after_manifest.take()
                                    {
                                        state.objects.insert(latest_path, latest);
                                    }
                                    ("200 OK", Vec::new())
                                }
                            }
                            "GET" => state
                                .objects
                                .get(&path)
                                .cloned()
                                .map_or(("404 Not Found", Vec::new()), |body| ("200 OK", body)),
                            "HEAD" => {
                                if state.objects.contains_key(&path) {
                                    ("200 OK", Vec::new())
                                } else {
                                    ("404 Not Found", Vec::new())
                                }
                            }
                            _ => ("405 Method Not Allowed", Vec::new()),
                        };
                        let header = format!(
                            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            response_body.len()
                        );
                        stream.write_all(header.as_bytes()).unwrap();
                        if method != "HEAD" {
                            stream.write_all(&response_body).unwrap();
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(error) => panic!("mock object store accept failed: {error}"),
                }
            }
        });
        Self {
            address,
            state,
            stop,
            thread: Some(thread),
        }
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.address)
    }

    fn puts(&self) -> Vec<String> {
        self.state.lock().unwrap().puts.clone()
    }

    fn object(&self, path: &str) -> Vec<u8> {
        self.state.lock().unwrap().objects[path].clone()
    }

    fn insert(&self, path: impl Into<String>, bytes: Vec<u8>) {
        self.state
            .lock()
            .unwrap()
            .objects
            .insert(path.into(), bytes);
    }

    fn replace_latest_after_manifest(&self, path: impl Into<String>, bytes: Vec<u8>) {
        self.state.lock().unwrap().replace_latest_after_manifest = Some((path.into(), bytes));
    }

    fn fail_put_suffix(&self, suffix: impl Into<String>) {
        self.state.lock().unwrap().fail_put_suffix = Some(suffix.into());
    }
}

impl Drop for MockObjectStore {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn read_http_request(stream: &mut std::net::TcpStream) -> (String, String, Vec<u8>) {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut chunk).unwrap();
        assert!(count > 0, "HTTP request ended before headers");
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]);
    let mut lines = headers.lines();
    let request_line = lines.next().unwrap();
    let mut request = request_line.split_whitespace();
    let method = request.next().unwrap().to_owned();
    let path = request.next().unwrap().to_owned();
    let content_length = lines
        .find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
        })
        .unwrap_or(0);
    while bytes.len() < header_end + content_length {
        let count = stream.read(&mut chunk).unwrap();
        assert!(count > 0, "HTTP request body ended early");
        bytes.extend_from_slice(&chunk[..count]);
    }
    (
        method,
        path,
        bytes[header_end..header_end + content_length].to_vec(),
    )
}

fn prepare_publish_app(directory: &TestDirectory, base: &str) -> [u8; 32] {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use ed25519_dalek::SigningKey;
    use sha2::{Digest as _, Sha256};

    let release_dir = directory
        .path()
        .join("dist/desktop/stable/1.0.1/12/aarch64-apple-darwin");
    fs::create_dir_all(&release_dir).unwrap();
    let zip_name = "desktop-1.0.1-12-aarch64.app.zip";
    let dmg_name = "desktop-1.0.1-12-aarch64.dmg";
    fs::write(release_dir.join(zip_name), b"appzip").unwrap();
    fs::write(release_dir.join(dmg_name), b"dmg").unwrap();
    fs::write(
        release_dir.join("artifact.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "app_id": "com.example.desktop",
            "channel": "stable",
            "version": "1.0.1",
            "build_number": 12,
            "target": "aarch64-apple-darwin",
            "artifacts": [
                {"kind":"macos_app_zip","file_name":zip_name,"sha256":format!("{:x}", Sha256::digest(b"appzip")),"size":6},
                {"kind":"macos_dmg","file_name":dmg_name,"sha256":format!("{:x}", Sha256::digest(b"dmg")),"size":3}
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    let seed = [7_u8; 32];
    let signing = SigningKey::from_bytes(&seed);
    fs::create_dir_all(directory.path().join(".secrets")).unwrap();
    fs::write(
        directory.path().join(".secrets/update.key"),
        format!("desktop-main:ed25519:{}\n", STANDARD.encode(seed)),
    )
    .unwrap();
    let public_key = STANDARD.encode(signing.verifying_key().to_bytes());
    fs::write(
        directory.path().join("nexora.toml"),
        format!(
            r#"schema_version = 1

[publish.targets.local]
provider = "s3"
endpoint = "{base}"
bucket = "desktop-releases"
region = "us-east-1"
force_path_style = true
public_base_url = "{base}/desktop-releases"
allow_insecure_http = true

[apps.desktop]
package = "desktop"
app_id = "com.example.desktop"
display_name = "Desktop"
publish_target = "local"
object_prefix = "e2e-test"

[apps.desktop.branding]
application_logo = "assets/logos/desktop/logo-icon-128.png"
icon_source = "assets/logos/desktop/logo-icon-source.png"
managed = true

[apps.desktop.release]
channel = "stable"
version = "1.0.1"
build_number = 12
minimum_supported_version = "0.0.0"
signing_key_file = ".secrets/update.key"

[apps.desktop.updater]
enabled = true
feed_url = "{base}/desktop-releases/e2e-test/desktop/stable/latest.json"
channels = ["stable"]
trusted_public_keys = ["desktop-main:ed25519:{public_key}"]
signing_key_env = "FALLBACK_SIGNING_KEY"

[apps.desktop.targets]
required = ["aarch64-apple-darwin"]

[apps.desktop.platforms.macos]
icon = "assets/logos/desktop/logo-icon.icns"
signing = "ad_hoc"
notarize = false

[apps.desktop.platforms.windows]
icon = "assets/logos/desktop/logo-icon.ico"

[apps.desktop.platforms.linux]
icons = ["assets/logos/desktop/logo-icon-128.png"]
"#
        ),
    )
    .unwrap();
    seed
}

fn signed_remote_manifest(seed: [u8; 32], sequence: u64) -> Vec<u8> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use ed25519_dalek::{Signer as _, SigningKey};
    use serde::Serialize;

    #[derive(Serialize)]
    struct Payload<'a> {
        manifest_sequence: u64,
        app_id: &'a str,
        channel: &'a str,
        version: &'a str,
        build_number: u64,
        minimum_supported_version: &'a str,
        published_at: i64,
        status: &'a str,
        notes_url: Option<&'a str>,
        artifacts: Vec<serde_json::Value>,
    }
    #[derive(Serialize)]
    struct Signature<'a> {
        key_id: &'a str,
        algorithm: &'a str,
        signature: String,
    }
    #[derive(Serialize)]
    struct Envelope<'a> {
        schema_version: u32,
        payload: Payload<'a>,
        signatures: Vec<Signature<'a>>,
    }
    let payload = Payload {
        manifest_sequence: sequence,
        app_id: "com.example.desktop",
        channel: "stable",
        version: "1.0.0",
        build_number: 1,
        minimum_supported_version: "0.0.0",
        published_at: 1,
        status: "available",
        notes_url: None,
        artifacts: Vec::new(),
    };
    let bytes = serde_json::to_vec(&payload).unwrap();
    let signature = SigningKey::from_bytes(&seed).sign(&bytes);
    serde_json::to_vec(&Envelope {
        schema_version: 1,
        payload,
        signatures: vec![Signature {
            key_id: "desktop-main",
            algorithm: "ed25519",
            signature: STANDARD.encode(signature.to_bytes()),
        }],
    })
    .unwrap()
}

fn expected_single_manifest(project_name: &str) -> String {
    askama_source(SINGLE_MANIFEST_TEMPLATE)
        .replace("{{ project_name }}", project_name)
        .replace("{{ nexora_source }}", &expected_nexora_source())
}

fn expected_workspace_manifest(project_name: &str, account_enabled: bool) -> String {
    render_account_condition(WORKSPACE_MANIFEST_TEMPLATE, account_enabled)
        .replace("{{ project_name }}", project_name)
        .replace("{{ nexora_source }}", &expected_nexora_source())
}

fn expected_nexora_source() -> String {
    format!(
        "git = \"{}\", tag = \"v{}\"",
        env!("CARGO_PKG_REPOSITORY"),
        env!("CARGO_PKG_VERSION")
    )
}

fn expected_desktop_manifest(project_name: &str, account_enabled: bool) -> String {
    render_account_condition(DESKTOP_MANIFEST_TEMPLATE, account_enabled)
        .replace("{{ project_name }}", project_name)
}

fn expected_main(project_name: &str, account_enabled: bool, workspace: bool) -> String {
    const START: &str = "{%- if account_enabled -%}\n";
    const ELSE: &str = "\n{%- else -%}\n";
    const END: &str = "\n{%- endif -%}";

    let source = askama_source(MAIN_TEMPLATE);
    let template = source
        .strip_prefix(START)
        .expect("main.rs 条件模板必须以 account_enabled 分支开始")
        .strip_suffix(END)
        .expect("main.rs 条件模板必须闭合");
    let (enabled, disabled) = template
        .split_once(ELSE)
        .expect("main.rs 条件模板必须包含无 Account 分支");
    let rendered = if account_enabled {
        enabled.to_owned()
    } else {
        disabled.to_owned()
    };
    let logo_path = if workspace {
        format!("../../../assets/logos/{project_name}/logo-icon-128.png")
    } else {
        format!("../assets/logos/{project_name}/logo-icon-128.png")
    };
    rendered
        .replace("{{ project_name }}", project_name)
        .replace("{{ logo_path }}", &logo_path)
}

fn render_account_condition(template: &str, account_enabled: bool) -> String {
    const START: &str = "{% if account_enabled %}";
    const ELSE: &str = "{% else %}";
    const END: &str = "{% endif %}";

    let mut rendered = askama_source(template);
    while let Some(start) = rendered.find(START) {
        let content_start = start + START.len();
        let end = rendered[content_start..]
            .find(END)
            .map(|end| content_start + end)
            .expect("account_enabled 条件模板必须闭合");
        let block = &rendered[content_start..end];
        let (enabled, disabled) = block.split_once(ELSE).unwrap_or((block, ""));
        let replacement = if account_enabled {
            enabled.to_owned()
        } else {
            disabled.to_owned()
        };
        rendered.replace_range(start..end + END.len(), replacement.as_str());
    }
    rendered
}

fn askama_source(template: &str) -> String {
    let normalized = template.replace("\r\n", "\n").replace('\r', "");
    normalized
        .strip_suffix('\n')
        .unwrap_or(&normalized)
        .to_owned()
}

fn assert_valid_manifest(path: &Path) {
    let contents = fs::read_to_string(path).expect("应能读取生成的 Cargo manifest");
    assert!(
        !contents.contains('\r'),
        "生成的 Cargo manifest 必须使用 LF 行尾：{}",
        path.display()
    );
    contents
        .parse::<toml_edit::DocumentMut>()
        .unwrap_or_else(|error| {
            panic!(
                "生成的 Cargo manifest 语法无效：{}：{error}",
                path.display()
            )
        });
}

fn assert_portable_nexora_dependency(path: &Path) {
    let contents = fs::read_to_string(path).expect("应能读取生成的 Cargo manifest");
    assert!(contents.contains(&expected_nexora_source()));
    assert!(!contents.contains(env!("CARGO_MANIFEST_DIR")));
    assert!(!contents.contains("nexora = { path ="));
}

fn collect_relative_files(root: &Path) -> Vec<PathBuf> {
    fn visit(root: &Path, directory: &Path, files: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(directory).expect("应能读取 Skill 目录") {
            let path = entry.expect("应能读取 Skill 目录项").path();
            if path.is_dir() {
                visit(root, &path, files);
            } else {
                files.push(
                    path.strip_prefix(root)
                        .expect("Skill 文件应位于根目录内")
                        .to_path_buf(),
                );
            }
        }
    }

    let mut files = Vec::new();
    visit(root, root, &mut files);
    files.sort();
    files
}

fn assert_generated_skills(project: &Path) {
    let template_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates/skills");
    let generated_root = project.join(".agents/skills");
    let template_files = collect_relative_files(&template_root);
    let generated_files = collect_relative_files(&generated_root);

    assert_eq!(generated_files, template_files);
    assert!(
        generated_files.contains(&PathBuf::from("develop-nexora-apps/SKILL.md")),
        "生成项目应包含 Nexora 框架 Skill"
    );
    assert!(
        generated_files.contains(&PathBuf::from("publish-nexora-release/SKILL.md")),
        "生成项目应包含 Nexora 版本发布 Skill"
    );
    assert!(
        generated_files.contains(&PathBuf::from("refine-implementation-spec/SKILL.md")),
        "生成项目应包含需求澄清与实施规格 Skill"
    );
    for relative_path in template_files {
        assert_eq!(
            fs::read(generated_root.join(&relative_path)).unwrap(),
            fs::read(template_root.join(&relative_path)).unwrap(),
            "Skill 模板应原样写入：{}",
            relative_path.display()
        );
    }
}

#[test]
fn packaged_skill_templates_match_the_workspace_agent_skills() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source_root = manifest.join("../../.agents/skills");
    let template_root = manifest.join("templates/skills");
    if !source_root.is_dir() {
        return;
    }
    let source_files = collect_relative_files(&source_root);
    let template_files = collect_relative_files(&template_root);

    assert_eq!(template_files, source_files);
    for relative_path in source_files {
        assert_eq!(
            fs::read(source_root.join(&relative_path)).unwrap(),
            fs::read(template_root.join(&relative_path)).unwrap(),
            "发布模板必须与仓库 Skill 保持一致：{}",
            relative_path.display()
        );
    }
}

#[test]
fn help_and_version_are_available() {
    let directory = TestDirectory::new("help-version");

    let help = directory.run(&["--help"]);
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("Usage: nexora"));
    assert!(String::from_utf8_lossy(&help.stdout).contains("create"));
    assert!(String::from_utf8_lossy(&help.stdout).contains("init"));
    assert!(String::from_utf8_lossy(&help.stdout).contains("build"));
    assert!(String::from_utf8_lossy(&help.stdout).contains("doctor"));
    assert!(String::from_utf8_lossy(&help.stdout).contains("lint"));

    let build_help = directory.run(&["help", "build"]);
    assert!(build_help.status.success());
    let build_help = String::from_utf8_lossy(&build_help.stdout);
    assert!(build_help.contains("build [OPTIONS]"));
    assert!(build_help.contains("--app <APP>"));
    for removed in [
        "--package",
        "--app-name",
        "--app-version",
        "--build-number",
        "--channel",
        "--mode",
        "--sign",
        "--sign-identity",
        "--notary-profile",
        "--skip-notarize",
        "--signing-key-file",
        "--skip-dmg",
    ] {
        assert!(!build_help.contains(removed), "仍显示旧参数 {removed}");
    }

    let publish_help = directory.run(&["help", "publish"]);
    assert!(publish_help.status.success());
    let publish_help = String::from_utf8_lossy(&publish_help.stdout);
    for expected in ["--app <APP>", "--all", "--dry-run", "--yes"] {
        assert!(publish_help.contains(expected));
    }
    for removed in [
        "--app-version",
        "--build-number",
        "--manifest-sequence",
        "--channel",
        "--signing-key-file",
        "--minimum-supported-version",
    ] {
        assert!(!publish_help.contains(removed), "仍显示旧参数 {removed}");
    }

    let create_help = directory.run(&["help", "create"]);
    assert!(create_help.status.success());
    let create_help = String::from_utf8_lossy(&create_help.stdout);
    assert!(create_help.contains("create [OPTIONS] [name]"));
    assert!(create_help.contains("--layout <LAYOUT>"));
    assert!(create_help.contains("single, workspace"));
    assert!(create_help.contains("--features <FEATURES>"));
    assert!(create_help.contains("account"));

    let init_help = directory.run(&["help", "init"]);
    assert!(init_help.status.success());
    let init_help = String::from_utf8_lossy(&init_help.stdout);
    assert!(init_help.contains("init [OPTIONS] [path]"));
    assert!(init_help.contains("--layout <LAYOUT>"));
    assert!(init_help.contains("--features <FEATURES>"));

    for flag in ["--version", "-v"] {
        let version = directory.run(&[flag]);
        assert!(version.status.success());
        assert_eq!(
            String::from_utf8_lossy(&version.stdout),
            format!("nexora {}\n", env!("CARGO_PKG_VERSION"))
        );
    }

    let version_command = directory.run(&["version"]);
    assert!(version_command.status.success());
    assert_eq!(
        String::from_utf8_lossy(&version_command.stdout),
        format!("nexora {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn publish_dry_run_signs_latest_from_existing_artifact_metadata() {
    let directory = TestDirectory::new("publish-dry-run");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        for stream in listener.incoming().take(4) {
            let mut stream = stream.unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request);
            stream
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
        }
    });
    let release_dir = directory
        .path()
        .join("dist/desktop/stable")
        .join("1.0.1")
        .join("12/aarch64-apple-darwin");
    fs::create_dir_all(&release_dir).unwrap();
    let zip_name = "desktop-1.0.1-12-aarch64.app.zip";
    let dmg_name = "desktop-1.0.1-12-aarch64.dmg";
    fs::write(release_dir.join(zip_name), b"appzip").unwrap();
    fs::write(release_dir.join(dmg_name), b"dmg").unwrap();
    use sha2::{Digest as _, Sha256};
    let dmg_sha = format!("{:x}", Sha256::digest(b"dmg"));
    fs::write(
        release_dir.join("artifact.json"),
        format!(r#"{{
  "schema_version": 1,
  "app_id": "com.example.desktop",
  "channel": "stable",
  "version": "1.0.1",
  "build_number": 12,
  "target": "aarch64-apple-darwin",
  "artifacts": [
    {{"kind":"macos_app_zip","file_name":"{zip_name}","sha256":"794f396be329ce58e99c9084550e92f52c2799a83a4ae46e6fcd6efde6b1a922","size":6}},
    {{"kind":"macos_dmg","file_name":"{dmg_name}","sha256":"{dmg_sha}","size":3}}
  ]
}}
"#),
    )
    .unwrap();
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use ed25519_dalek::SigningKey;
    let seed = [7_u8; 32];
    let public_key = STANDARD.encode(SigningKey::from_bytes(&seed).verifying_key().to_bytes());
    fs::write(
        directory.path().join("nexora.toml"),
        format!(
            r#"schema_version = 1

[publish.targets.local]
provider = "s3"
endpoint = "http://{address}"
bucket = "desktop-releases"
region = "us-east-1"
force_path_style = true
public_base_url = "http://{address}/desktop-releases"
allow_insecure_http = true

[apps.desktop]
package = "desktop"
app_id = "com.example.desktop"
display_name = "Desktop"
publish_target = "local"
object_prefix = "e2e-test"

[apps.desktop.branding]
application_logo = "assets/logos/desktop/logo-icon-128.png"
icon_source = "assets/logos/desktop/logo-icon-source.png"
managed = true

[apps.desktop.release]
channel = "stable"
version = "1.0.1"
build_number = 12
minimum_supported_version = "0.0.0"

[apps.desktop.updater]
enabled = true
feed_url = "http://{address}/desktop-releases/e2e-test/desktop/stable/latest.json"
channels = ["stable"]
trusted_public_keys = ["desktop-main:ed25519:{public_key}"]
signing_key_env = "NEXORA_TEST_SIGNING_KEY"

[apps.desktop.targets]
required = ["aarch64-apple-darwin"]

[apps.desktop.platforms.macos]
icon = "assets/logos/desktop/logo-icon.icns"
signing = "ad_hoc"
notarize = false

[apps.desktop.platforms.windows]
icon = "assets/logos/desktop/logo-icon.ico"

[apps.desktop.platforms.linux]
icons = ["assets/logos/desktop/logo-icon-128.png"]
"#
        ),
    )
    .unwrap();

    let output = directory.run_with_env(
        &["publish", "--app", "desktop", "--dry-run"],
        &[(
            "NEXORA_TEST_SIGNING_KEY",
            "desktop-main:ed25519:BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=",
        )],
    );

    assert!(
        output.status.success(),
        "publish dry-run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    server.join().unwrap();
    let latest_path = directory.path().join("dist/desktop/stable/latest.json");
    assert!(!latest_path.exists(), "dry-run 不应写本地 latest.json");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Manifest sequence：1（自动计算）"));
    assert!(stdout.contains("dry-run: 已完成远端读取与全部预检"));
}

#[test]
fn publish_uploads_zip_dmg_aliases_and_latest_in_required_order() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use ed25519_dalek::SigningKey;
    use sha2::{Digest as _, Sha256};

    let directory = TestDirectory::new("publish-order");
    let store = MockObjectStore::new();
    let base = store.base_url();
    let release_dir = directory
        .path()
        .join("dist/desktop/stable/1.0.1/12/aarch64-apple-darwin");
    fs::create_dir_all(&release_dir).unwrap();
    let zip_name = "desktop-1.0.1-12-aarch64.app.zip";
    let dmg_name = "desktop-1.0.1-12-aarch64.dmg";
    fs::write(release_dir.join(zip_name), b"appzip").unwrap();
    fs::write(release_dir.join(dmg_name), b"dmg").unwrap();
    fs::write(
        release_dir.join("artifact.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "app_id": "com.example.desktop",
            "channel": "stable",
            "version": "1.0.1",
            "build_number": 12,
            "target": "aarch64-apple-darwin",
            "artifacts": [
                {
                    "kind": "macos_app_zip",
                    "file_name": zip_name,
                    "sha256": format!("{:x}", Sha256::digest(b"appzip")),
                    "size": 6
                },
                {
                    "kind": "macos_dmg",
                    "file_name": dmg_name,
                    "sha256": format!("{:x}", Sha256::digest(b"dmg")),
                    "size": 3
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    let seed = [7_u8; 32];
    let signing = SigningKey::from_bytes(&seed);
    fs::create_dir_all(directory.path().join(".secrets")).unwrap();
    fs::write(
        directory.path().join(".secrets/update.key"),
        format!("desktop-main:ed25519:{}\n", STANDARD.encode(seed)),
    )
    .unwrap();
    let public_key = STANDARD.encode(signing.verifying_key().to_bytes());
    fs::write(
        directory.path().join("nexora.toml"),
        format!(
            r#"schema_version = 1

[publish.targets.local]
provider = "s3"
endpoint = "{base}"
bucket = "desktop-releases"
region = "us-east-1"
force_path_style = true
public_base_url = "{base}/desktop-releases"
allow_insecure_http = true

[apps.desktop]
package = "desktop"
app_id = "com.example.desktop"
display_name = "Desktop"
publish_target = "local"
object_prefix = "e2e-test"

[apps.desktop.branding]
application_logo = "assets/logos/desktop/logo-icon-128.png"
icon_source = "assets/logos/desktop/logo-icon-source.png"
managed = true

[apps.desktop.release]
channel = "stable"
version = "1.0.1"
build_number = 12
minimum_supported_version = "0.0.0"
signing_key_file = ".secrets/update.key"

[apps.desktop.updater]
enabled = true
feed_url = "{base}/desktop-releases/e2e-test/desktop/stable/latest.json"
channels = ["stable"]
trusted_public_keys = ["desktop-main:ed25519:{public_key}"]
signing_key_env = "UNUSED_SIGNING_KEY"

[apps.desktop.targets]
required = ["aarch64-apple-darwin"]

[apps.desktop.platforms.macos]
icon = "assets/logos/desktop/logo-icon.icns"
signing = "ad_hoc"
notarize = false

[apps.desktop.platforms.windows]
icon = "assets/logos/desktop/logo-icon.ico"

[apps.desktop.platforms.linux]
icons = ["assets/logos/desktop/logo-icon-128.png"]
"#
        ),
    )
    .unwrap();

    let output = directory.run_with_env(
        &["publish", "--app", "desktop", "--yes"],
        &[
            ("AWS_ACCESS_KEY_ID", "test-access-key"),
            ("AWS_SECRET_ACCESS_KEY", "test-secret-key"),
        ],
    );
    assert!(
        output.status.success(),
        "publish failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let prefix = "/desktop-releases/e2e-test/desktop/stable";
    assert_eq!(
        store.puts(),
        vec![
            format!("{prefix}/releases/1.0.1/12/aarch64-apple-darwin/{zip_name}"),
            format!("{prefix}/releases/1.0.1/12/aarch64-apple-darwin/{dmg_name}"),
            format!("{prefix}/manifests/1.json"),
            format!("{prefix}/latest-aarch64.dmg"),
            format!("{prefix}/latest.dmg"),
            format!("{prefix}/latest.json"),
        ]
    );
    let latest: serde_json::Value =
        serde_json::from_slice(&store.object(&format!("{prefix}/latest.json"))).unwrap();
    assert_eq!(latest["payload"]["manifest_sequence"], 1);
    assert_eq!(latest["payload"]["artifacts"].as_array().unwrap().len(), 1);
    assert_eq!(latest["payload"]["artifacts"][0]["kind"], "macos_app_zip");
    assert!(
        !serde_json::to_string(&latest["payload"]["artifacts"])
            .unwrap()
            .contains("dmg")
    );
}

#[test]
fn publish_uses_verified_remote_sequence_plus_one() {
    let directory = TestDirectory::new("publish-next-sequence");
    let store = MockObjectStore::new();
    let base = store.base_url();
    let seed = prepare_publish_app(&directory, &base);
    let latest_path = "/desktop-releases/e2e-test/desktop/stable/latest.json";
    store.insert(latest_path, signed_remote_manifest(seed, 4));

    let output = directory.run(&["publish", "--app", "desktop", "--dry-run"]);
    assert!(
        output.status.success(),
        "dry-run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Manifest sequence：5（自动计算）"));
    assert!(store.puts().is_empty());
}

#[test]
fn publish_rejects_unverifiable_remote_manifest() {
    let directory = TestDirectory::new("publish-bad-remote");
    let store = MockObjectStore::new();
    let base = store.base_url();
    prepare_publish_app(&directory, &base);
    let latest_path = "/desktop-releases/e2e-test/desktop/stable/latest.json";
    store.insert(latest_path, signed_remote_manifest([8_u8; 32], 4));

    let output = directory.run(&["publish", "--app", "desktop", "--dry-run"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("无法由 trusted_public_keys 验签"));
    assert!(store.puts().is_empty());
}

#[test]
fn publish_rejects_concurrent_sequence_change_before_mutable_uploads() {
    let directory = TestDirectory::new("publish-concurrent");
    let store = MockObjectStore::new();
    let base = store.base_url();
    let seed = prepare_publish_app(&directory, &base);
    let latest_path = "/desktop-releases/e2e-test/desktop/stable/latest.json";
    store.replace_latest_after_manifest(latest_path, signed_remote_manifest(seed, 9));

    let output = directory.run_with_env(
        &["publish", "--app", "desktop", "--yes"],
        &[
            ("AWS_ACCESS_KEY_ID", "test-access-key"),
            ("AWS_SECRET_ACCESS_KEY", "test-secret-key"),
        ],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("发布期间"));
    let puts = store.puts();
    assert_eq!(
        puts.len(),
        3,
        "只允许写入两个版本化产物和 sequence manifest"
    );
    assert!(puts[2].ends_with("/manifests/1.json"));
    assert!(!puts.iter().any(|path| path.ends_with("/latest.json")));
    assert!(!puts.iter().any(|path| path.ends_with("/latest.dmg")));
}

#[test]
fn publish_rejects_existing_immutable_object_before_upload() {
    let directory = TestDirectory::new("publish-immutable");
    let store = MockObjectStore::new();
    let base = store.base_url();
    prepare_publish_app(&directory, &base);
    let existing = "/desktop-releases/e2e-test/desktop/stable/releases/1.0.1/12/aarch64-apple-darwin/desktop-1.0.1-12-aarch64.app.zip";
    store.insert(existing, b"already-there".to_vec());

    let output = directory.run(&["publish", "--app", "desktop", "--dry-run"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("immutable 对象已存在"));
    assert!(store.puts().is_empty());
}

#[test]
fn latest_dmg_failure_prevents_latest_json_upload() {
    let directory = TestDirectory::new("publish-latest-dmg-failure");
    let store = MockObjectStore::new();
    let base = store.base_url();
    prepare_publish_app(&directory, &base);
    store.fail_put_suffix("/latest-aarch64.dmg");

    let output = directory.run_with_env(
        &["publish", "--app", "desktop", "--yes"],
        &[
            ("AWS_ACCESS_KEY_ID", "test-access-key"),
            ("AWS_SECRET_ACCESS_KEY", "test-secret-key"),
        ],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("latest-aarch64.dmg"));
    let puts = store.puts();
    assert!(
        puts.iter()
            .any(|path| path.ends_with("/latest-aarch64.dmg"))
    );
    assert!(!puts.iter().any(|path| path.ends_with("/latest.json")));
}

#[test]
fn configured_missing_signing_key_file_never_falls_back_to_environment() {
    let directory = TestDirectory::new("publish-missing-key-file");
    let store = MockObjectStore::new();
    let base = store.base_url();
    let seed = prepare_publish_app(&directory, &base);
    fs::remove_file(directory.path().join(".secrets/update.key")).unwrap();
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let fallback = format!("desktop-main:ed25519:{}", STANDARD.encode(seed));

    let output = directory.run_with_env(
        &["publish", "--app", "desktop", "--dry-run"],
        &[("FALLBACK_SIGNING_KEY", &fallback)],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("无法读取签名私钥文件"));
    assert!(store.puts().is_empty());
}

#[test]
fn noninteractive_publish_requires_yes_before_writes() {
    let directory = TestDirectory::new("publish-requires-yes");
    let output = directory.run(&["publish", "--app", "desktop"]);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("必须提供 `--yes`"));
}

#[test]
fn create_without_arguments_uses_non_tty_defaults() {
    let directory = TestDirectory::new("create-defaults");

    let output = directory.run(&["create"]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let project = directory.path().join("nexora-app");
    assert!(project.join("Cargo.toml").is_file());
    assert!(project.join("src/main.rs").is_file());
    assert!(!project.join("apps").exists());
    assert_generated_skills(&project);
}

#[test]
fn init_without_arguments_uses_current_directory_in_non_tty_mode() {
    let directory = TestDirectory::new("init-defaults");

    let output = directory.run(&["init"]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(directory.path().join("Cargo.toml").is_file());
    assert!(directory.path().join("src/main.rs").is_file());
    assert!(!directory.path().join("apps").exists());
    assert_generated_skills(directory.path());
}

#[test]
fn create_defaults_to_a_single_package_project() {
    let directory = TestDirectory::new("create-single");

    let output = directory.run(&["create", "demo-app"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let project = directory.path().join("demo-app");
    assert_valid_manifest(&project.join("Cargo.toml"));
    assert_portable_nexora_dependency(&project.join("Cargo.toml"));
    assert_eq!(
        fs::read_to_string(project.join("Cargo.toml")).unwrap(),
        expected_single_manifest("demo-app")
    );
    assert_eq!(
        fs::read_to_string(project.join(".gitignore")).unwrap(),
        askama_source(GITIGNORE_TEMPLATE)
    );
    let main = fs::read_to_string(project.join("src/main.rs")).unwrap();
    assert_eq!(main, expected_main("demo-app", false, false));
    assert!(main.contains("#[derive(RustEmbed)]"));
    assert!(main.contains(".application_assets(AppAssets)"));
    assert!(project.join("assets/icons").is_dir());
    assert_eq!(
        fs::read_to_string(project.join("src/features.rs")).unwrap(),
        askama_source(FEATURES_TEMPLATE)
    );
    assert_eq!(
        fs::read_to_string(project.join("src/features/home.rs")).unwrap(),
        askama_source(HOME_FEATURE_TEMPLATE)
    );
    let readme = fs::read_to_string(project.join("README.md")).unwrap();
    assert!(!readme.contains('\r'));
    assert!(readme.contains("# demo-app"));
    assert!(readme.contains("cargo run"));
    assert!(!readme.contains("cargo run -p desktop"));
    assert_eq!(
        fs::read_to_string(project.join("AGENTS.md")).unwrap(),
        askama_source(AGENTS_TEMPLATE)
    );
    assert!(!project.join("apps").exists());
    assert_generated_skills(&project);

    assert!(main.contains("impl nexora::Application for DesktopApplication"));
    assert!(main.contains("DesktopApplication.run()"));
    assert!(HOME_FEATURE_TEMPLATE.contains("impl FeatureElement for HomeFeature"));
    for name in [
        "logo-icon-16.png",
        "logo-icon-24.png",
        "logo-icon-32.png",
        "logo-icon-48.png",
        "logo-icon-64.png",
        "logo-icon-128.png",
        "logo-icon-256.png",
        "logo-icon-512.png",
        "logo-icon-1024.png",
        "logo-icon-source.png",
        "logo-icon.icns",
        "logo-icon.ico",
    ] {
        assert!(project.join("assets/logos/demo-app").join(name).is_file());
    }
}

#[test]
fn icons_generate_is_app_scoped_and_deterministic() {
    use sha2::{Digest as _, Sha256};

    let directory = TestDirectory::new("icons-generate");
    let create = directory.run(&["create", "icons-app"]);
    assert!(create.status.success());
    let project = directory.path().join("icons-app");
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_nexora"))
            .args(["icons", "generate", "--app", "icons-app"])
            .current_dir(&project)
            .output()
            .unwrap()
    };

    let first = run();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let assets = project.join("assets/logos/icons-app");
    let digest = |name: &str| format!("{:x}", Sha256::digest(fs::read(assets.join(name)).unwrap()));
    let before = [
        digest("logo-icon-128.png"),
        digest("logo-icon.icns"),
        digest("logo-icon.ico"),
    ];
    let second = run();
    assert!(second.status.success());
    let after = [
        digest("logo-icon-128.png"),
        digest("logo-icon.icns"),
        digest("logo-icon.ico"),
    ];
    assert_eq!(before, after);
}

#[test]
fn icons_generate_protects_manual_resources_and_validates_source_size() {
    let directory = TestDirectory::new("icons-validation");
    let create = directory.run(&["create", "manual-icons"]);
    assert!(create.status.success());
    let project = directory.path().join("manual-icons");
    let config_path = project.join("nexora.toml");
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace("managed = true", "managed = false");
    fs::write(&config_path, config).unwrap();
    let run = |extra: &[&str]| {
        let mut args = vec!["icons", "generate", "--app", "manual-icons"];
        args.extend_from_slice(extra);
        Command::new(env!("CARGO_BIN_EXE_nexora"))
            .args(args)
            .current_dir(&project)
            .output()
            .unwrap()
    };

    let protected = run(&[]);
    assert!(!protected.status.success());
    assert!(String::from_utf8_lossy(&protected.stderr).contains("手工资源"));

    fs::copy(
        project.join("assets/logos/manual-icons/logo-icon-128.png"),
        project.join("assets/logos/manual-icons/logo-icon-source.png"),
    )
    .unwrap();
    let invalid = run(&["--force"]);
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("至少 1024×1024"));
}

#[test]
fn init_preserves_existing_app_brand_resource() {
    let directory = TestDirectory::new("init-brand-preserve");
    let project = directory.path().join("brand-app");
    let assets = project.join("assets/logos/brand-app");
    fs::create_dir_all(&assets).unwrap();
    fs::write(assets.join("logo-icon-source.png"), b"user-brand").unwrap();

    let output = directory.run(&["init", "brand-app"]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(assets.join("logo-icon-source.png")).unwrap(),
        b"user-brand"
    );
    assert!(assets.join("logo-icon.icns").is_file());
}

#[test]
fn create_can_generate_a_workspace_project() {
    let directory = TestDirectory::new("create-workspace");

    let output = directory.run(&["create", "workspace-app", "--layout", "workspace"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let project = directory.path().join("workspace-app");
    let desktop = project.join("apps/workspace-app");
    assert_valid_manifest(&project.join("Cargo.toml"));
    assert_portable_nexora_dependency(&project.join("Cargo.toml"));
    assert_valid_manifest(&desktop.join("Cargo.toml"));
    assert_eq!(
        fs::read_to_string(project.join("Cargo.toml")).unwrap(),
        expected_workspace_manifest("workspace-app", false)
    );
    assert_eq!(
        fs::read_to_string(project.join(".gitignore")).unwrap(),
        askama_source(GITIGNORE_TEMPLATE)
    );
    assert_eq!(
        fs::read_to_string(desktop.join("Cargo.toml")).unwrap(),
        expected_desktop_manifest("workspace-app", false)
    );
    assert_eq!(
        fs::read_to_string(desktop.join("src/main.rs")).unwrap(),
        expected_main("workspace-app", false, true)
    );
    let desktop_main = fs::read_to_string(desktop.join("src/main.rs")).unwrap();
    assert!(desktop_main.contains("#[derive(RustEmbed)]"));
    assert!(desktop_main.contains(".application_assets(AppAssets)"));
    assert!(desktop.join("assets/icons").is_dir());
    assert_eq!(
        fs::read_to_string(desktop.join("src/features.rs")).unwrap(),
        askama_source(FEATURES_TEMPLATE)
    );
    assert_eq!(
        fs::read_to_string(desktop.join("src/features/home.rs")).unwrap(),
        askama_source(HOME_FEATURE_TEMPLATE)
    );
    let readme = fs::read_to_string(project.join("README.md")).unwrap();
    assert!(!readme.contains('\r'));
    assert!(readme.contains("# workspace-app"));
    assert!(readme.contains("cargo run"));
    assert!(!readme.contains("cargo run -p desktop"));
    assert!(!project.join("src").exists());
    assert!(!project.join("apps/server").exists());
    assert!(!desktop.join("src/account.rs").exists());
    assert!(!desktop.join("src/config.rs").exists());
    assert!(!project.join("config").exists());
    assert_generated_skills(&project);
}

#[test]
fn init_single_preserves_existing_content() {
    let directory = TestDirectory::new("init-single");
    let project = directory.path().join("existing-app");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("README.md"), "keep me").unwrap();
    fs::write(project.join("AGENTS.md"), "keep my rules").unwrap();

    let output = directory.run(&["init", "existing-app"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(project.join("README.md")).unwrap(),
        "keep me"
    );
    assert_eq!(
        fs::read_to_string(project.join("AGENTS.md")).unwrap(),
        "keep my rules"
    );
    assert_eq!(
        fs::read_to_string(project.join("Cargo.toml")).unwrap(),
        expected_single_manifest("existing-app")
    );
    assert_portable_nexora_dependency(&project.join("Cargo.toml"));
    assert_eq!(
        fs::read_to_string(project.join("src/main.rs")).unwrap(),
        expected_main("existing-app", false, false)
    );
    assert_generated_skills(&project);
}

#[test]
fn init_can_generate_a_workspace_and_preserve_existing_content() {
    let directory = TestDirectory::new("init-workspace");
    let project = directory.path().join("existing-workspace");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("README.md"), "keep me").unwrap();

    let output = directory.run(&["init", "existing-workspace", "--layout", "workspace"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(project.join("README.md")).unwrap(),
        "keep me"
    );
    assert_eq!(
        fs::read_to_string(project.join("Cargo.toml")).unwrap(),
        expected_workspace_manifest("existing-workspace", false)
    );
    assert_portable_nexora_dependency(&project.join("Cargo.toml"));
    let desktop = project.join("apps/existing-workspace");
    assert_eq!(
        fs::read_to_string(desktop.join("Cargo.toml")).unwrap(),
        expected_desktop_manifest("existing-workspace", false)
    );
    assert_eq!(
        fs::read_to_string(desktop.join("src/main.rs")).unwrap(),
        expected_main("existing-workspace", false, true)
    );
    assert_generated_skills(&project);
}

#[test]
fn workspace_account_feature_generates_a_composable_server() {
    let directory = TestDirectory::new("workspace-account");

    let output = directory.run(&[
        "create",
        "fullstack-app",
        "--layout",
        "workspace",
        "--features",
        "account",
        "--features",
        "account",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let project = directory.path().join("fullstack-app");
    let desktop = project.join("apps/fullstack-app");
    assert_valid_manifest(&project.join("Cargo.toml"));
    assert_valid_manifest(&desktop.join("Cargo.toml"));
    assert_valid_manifest(&project.join("apps/server/Cargo.toml"));
    assert_eq!(
        fs::read_to_string(project.join("Cargo.toml")).unwrap(),
        expected_workspace_manifest("fullstack-app", true)
    );
    assert_eq!(
        fs::read_to_string(desktop.join("Cargo.toml")).unwrap(),
        expected_desktop_manifest("fullstack-app", true)
    );
    assert_eq!(
        fs::read_to_string(desktop.join("src/main.rs")).unwrap(),
        expected_main("fullstack-app", true, true)
    );
    assert!(!desktop.join("src/account.rs").exists());
    assert_eq!(
        fs::read_to_string(desktop.join("src/config.rs")).unwrap(),
        askama_source(DESKTOP_CONFIG_TEMPLATE)
    );
    assert_eq!(
        fs::read_to_string(project.join("apps/server/Cargo.toml")).unwrap(),
        askama_source(SERVER_MANIFEST_TEMPLATE)
    );
    assert_eq!(
        fs::read_to_string(project.join("apps/server/src/main.rs")).unwrap(),
        askama_source(SERVER_MAIN_TEMPLATE)
    );
    assert_eq!(
        fs::read_to_string(project.join("apps/server/src/config.rs")).unwrap(),
        askama_source(SERVER_CONFIG_TEMPLATE)
    );
    assert_eq!(
        fs::read_to_string(project.join("apps/server/src/routes.rs")).unwrap(),
        askama_source(SERVER_ROUTES_TEMPLATE)
    );
    assert_eq!(
        fs::read_to_string(project.join("config/server.toml")).unwrap(),
        askama_source(EXAMPLE_SERVER_CONFIG_TEMPLATE)
    );
    assert_eq!(
        fs::read_to_string(project.join("config/fullstack-app.toml")).unwrap(),
        askama_source(EXAMPLE_DESKTOP_CONFIG_TEMPLATE)
    );
    let desktop_config = fs::read_to_string(project.join("config/fullstack-app.toml")).unwrap();
    assert!(desktop_config.contains("[api]"));
    assert!(desktop_config.contains("endpoint = \"http://127.0.0.1:3000\""));
    assert!(desktop_config.contains("allow_insecure_http = false"));
    assert!(!desktop_config.contains("[account.api]"));
    assert!(desktop_config.contains("# OIDC Provider 的 issuer URL"));
    let server_config = fs::read_to_string(project.join("config/server.toml")).unwrap();
    assert!(server_config.contains("# HTTP 服务监听 IP"));
    assert!(server_config.contains("ip = \"127.0.0.1\""));
    assert!(server_config.contains("port = 3000"));
    assert!(!server_config.contains("bind ="));
    assert!(server_config.contains("# PostgreSQL 连接 URL"));
    assert!(server_config.contains("[setup]"));
    assert!(server_config.contains("project_id = \"replace-with-zitadel-project-id\""));
    assert!(
        server_config
            .contains("personal_access_token = \"replace-with-zitadel-service-account-pat\"")
    );
    assert!(!server_config.contains("initialize_empty_database"));
    let readme = fs::read_to_string(project.join("README.md")).unwrap();
    assert!(!readme.contains('\r'));
    assert!(readme.contains("cargo run -p server -- config/server.toml"));
    assert!(readme.contains("cargo run -- config/fullstack-app.toml"));
    assert_generated_skills(&project);

    let server_main = fs::read_to_string(project.join("apps/server/src/main.rs")).unwrap();
    syn::parse_file(server_main.as_str()).expect("生成的服务端入口必须是有效 Rust 源码");
    assert!(server_main.contains("use nexora::Server;"));
    assert!(server_main.contains("Server::new()"));
    assert!(server_main.contains("PgPoolOptions::new()"));
    assert!(server_main.contains("nexora::server::migrations()"));
    assert!(server_main.contains("Migrator::with_migrations(framework_migrations)"));
    assert!(server_main.contains(".run(&pool)"));
    assert!(server_main.contains("server.initialize(&settings, &pool, setup_secret)"));
    assert!(
        server_main
            .find("Migrator::with_migrations")
            .expect("生成入口必须组合迁移")
            < server_main
                .find("server.initialize")
                .expect("生成入口必须初始化 Server")
    );
    assert!(server_main.contains("Router::new()"));
    assert!(server_main.contains(".merge(server.routers())"));
    assert!(server_main.contains(".merge(routes::routers())"));
    assert!(server_main.contains("tokio::net::TcpListener::bind"));
    assert!(server_main.contains("(settings.server.ip, settings.server.port)"));
    assert!(server_main.contains("axum::serve(listener, app).await?"));
    assert!(!server_main.contains("server.run("));
    assert!(!server_main.contains("ServerOptions"));
    assert!(!server_main.contains("nexora::account::server::"));
    assert!(!server_main.contains("shutdown_signal"));
    let server_config_source =
        fs::read_to_string(project.join("apps/server/src/config.rs")).unwrap();
    syn::parse_file(server_config_source.as_str()).expect("生成的服务端配置必须是有效 Rust 源码");
    assert!(server_config_source.contains("#[nexora(account_server)]"));
    assert!(server_config_source.contains("pub(crate) setup: SetupSettings"));

    let desktop_main = fs::read_to_string(desktop.join("src/main.rs")).unwrap();
    syn::parse_file(desktop_main.as_str()).expect("生成的桌面入口必须是有效 Rust 源码");
    assert!(desktop_main.contains("nexora::config::initialize(None)"));
    assert!(desktop_main.contains("nexora::desktop::client_config(&settings, &settings.api)"));
    assert!(desktop_main.contains("AccountAuthenticator::new"));
    assert!(desktop_main.contains(".application_version(env!(\"CARGO_PKG_VERSION\"))"));
    assert!(desktop_main.contains("../../../assets/logos/fullstack-app/logo-icon-128.png"));
    assert!(desktop_main.contains("#[derive(RustEmbed)]"));
    assert!(desktop_main.contains(".application_assets(AppAssets)"));
    assert!(!desktop_main.contains("account_enabled"));
    assert!(desktop_main.contains("authenticator: AccountAuthenticator"));
    assert!(
        desktop_main
            .contains("nexora::desktop::install_authenticator(self.authenticator.clone(), cx)")
    );
    assert!(!desktop_main.contains("AccountRuntime"));
    assert!(!desktop_main.contains("begin_login"));
    let desktop_manifest = fs::read_to_string(desktop.join("Cargo.toml")).unwrap();
    assert!(desktop_manifest.contains("features = [\"desktop\", \"derive\"]"));
    assert!(desktop_manifest.contains("rust-embed = { workspace = true }"));
    assert!(!desktop_manifest.contains("account-client"));
    let server_manifest = fs::read_to_string(project.join("apps/server/Cargo.toml")).unwrap();
    assert!(server_manifest.contains("features = [\"server\", \"derive\"]"));
    assert!(server_manifest.contains("sqlx = { workspace = true }"));
    let workspace_manifest = fs::read_to_string(project.join("Cargo.toml")).unwrap();
    assert!(workspace_manifest.contains("rust-embed = { version = \"8.7.2\""));
    assert!(workspace_manifest.contains("features = [\"migrate\", \"postgres\""));
    assert!(!server_manifest.contains("account-server"));
    assert!(!server_manifest.contains("account-zitadel"));
    for logo in [
        "logo-icon-16.png",
        "logo-icon-128.png",
        "logo-icon-1024.png",
        "logo-icon-source.png",
        "logo-icon.icns",
        "logo-icon.ico",
    ] {
        assert!(
            project
                .join("assets/logos/fullstack-app")
                .join(logo)
                .is_file()
        );
    }
    let nexora_config = fs::read_to_string(project.join("nexora.toml")).unwrap();
    assert!(nexora_config.contains("[apps.fullstack-app.branding]"));
    assert!(nexora_config.contains("assets/logos/fullstack-app/logo-icon.icns"));
    assert!(desktop.join("assets/icons").is_dir());
    let desktop_config_source = fs::read_to_string(desktop.join("src/config.rs")).unwrap();
    assert!(desktop_config_source.contains("pub(crate) api:"));
    assert!(desktop_config_source.contains("#[nexora(account_client)]"));
}

#[test]
fn init_workspace_account_generates_all_agent_skills() {
    let directory = TestDirectory::new("init-workspace-account-skills");
    let project = directory.path().join("existing-account-workspace");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("README.md"), "keep me").unwrap();

    let output = directory.run(&[
        "init",
        "existing-account-workspace",
        "--layout",
        "workspace",
        "--features",
        "account",
    ]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(project.join("README.md")).unwrap(),
        "keep me"
    );
    assert!(project.join("apps/server/src/main.rs").is_file());
    assert!(project.join("config/server.toml").is_file());
    assert!(
        project
            .join("config/existing-account-workspace.toml")
            .is_file()
    );
    assert_generated_skills(&project);
}

#[test]
fn account_feature_accepts_comma_separated_values() {
    let directory = TestDirectory::new("account-comma-separated");

    let output = directory.run(&[
        "create",
        "comma-app",
        "--layout",
        "workspace",
        "--features",
        "account,account",
    ]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        directory
            .path()
            .join("comma-app/apps/server/src/main.rs")
            .is_file()
    );
}

#[test]
fn account_feature_requires_workspace_without_leaving_files() {
    let directory = TestDirectory::new("account-requires-workspace");

    let create = directory.run(&[
        "create",
        "single-app",
        "--layout",
        "single",
        "--features",
        "account",
    ]);
    assert!(!create.status.success());
    assert!(String::from_utf8_lossy(&create.stderr).contains("请改用 `--layout workspace`"));
    assert!(!directory.path().join("single-app").exists());

    let init = directory.run(&[
        "init",
        "single-init",
        "--layout",
        "single",
        "--features",
        "account",
    ]);
    assert!(!init.status.success());
    assert!(String::from_utf8_lossy(&init.stderr).contains("请改用 `--layout workspace`"));
    assert!(!directory.path().join("single-init").exists());
}

#[test]
fn account_workspace_rejects_the_reserved_server_project_name() {
    let directory = TestDirectory::new("account-reserved-server-name");

    for name in ["server", "Server"] {
        let output = directory.run(&[
            "create",
            name,
            "--layout",
            "workspace",
            "--features",
            "account",
        ]);

        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("保留包名"));
        assert!(!directory.path().join(name).exists());
    }
}

#[test]
fn account_without_layout_uses_workspace_in_non_tty_mode() {
    let directory = TestDirectory::new("account-auto-workspace");

    let output = directory.run(&["create", "account-app", "--features", "account"]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("项目结构已自动调整为 workspace"));
    let project = directory.path().join("account-app");
    assert!(!project.join("apps/account-app/src/account.rs").exists());
    assert!(project.join("apps/account-app/src/config.rs").is_file());
    assert!(project.join("apps/server/src/main.rs").is_file());
}

#[test]
fn account_workspace_failure_does_not_leave_partial_scaffold() {
    let directory = TestDirectory::new("account-no-partial");
    let project = directory.path().join("server-route-exists");
    fs::create_dir_all(project.join("apps/server/src")).unwrap();
    fs::write(
        project.join("apps/server/src/routes.rs"),
        "server route sentinel",
    )
    .unwrap();

    let output = directory.run(&[
        "init",
        "server-route-exists",
        "--layout",
        "workspace",
        "--features",
        "account",
    ]);

    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(project.join("apps/server/src/routes.rs")).unwrap(),
        "server route sentinel"
    );
    assert!(!project.join("Cargo.toml").exists());
    assert!(!project.join("apps/server-route-exists").exists());
    assert!(!project.join("config").exists());
}

#[test]
fn init_never_overwrites_single_package_files() {
    let directory = TestDirectory::new("single-no-overwrite");

    let manifest_project = directory.path().join("manifest-exists");
    fs::create_dir(&manifest_project).unwrap();
    fs::write(manifest_project.join("Cargo.toml"), "manifest sentinel").unwrap();
    let manifest_output = directory.run(&["init", "manifest-exists"]);
    assert!(!manifest_output.status.success());
    assert_eq!(
        fs::read_to_string(manifest_project.join("Cargo.toml")).unwrap(),
        "manifest sentinel"
    );
    assert!(!manifest_project.join("src/main.rs").exists());

    let main_project = directory.path().join("main-exists");
    fs::create_dir_all(main_project.join("src")).unwrap();
    fs::write(main_project.join("src/main.rs"), "main sentinel").unwrap();
    let main_output = directory.run(&["init", "main-exists"]);
    assert!(!main_output.status.success());
    assert_eq!(
        fs::read_to_string(main_project.join("src/main.rs")).unwrap(),
        "main sentinel"
    );
    assert!(!main_project.join("Cargo.toml").exists());
}

#[test]
fn init_never_overwrites_an_existing_agent_skill() {
    let directory = TestDirectory::new("skill-no-overwrite");
    let project = directory.path().join("existing-skill");
    let skill = project.join(".agents/skills/develop-nexora-apps/SKILL.md");
    fs::create_dir_all(skill.parent().unwrap()).unwrap();
    fs::write(&skill, "skill sentinel").unwrap();

    let output = directory.run(&["init", "existing-skill"]);

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains(".agents/skills/develop-nexora-apps/SKILL.md")
    );
    assert_eq!(fs::read_to_string(skill).unwrap(), "skill sentinel");
    assert!(!project.join("Cargo.toml").exists());
}

#[test]
fn workspace_failures_do_not_leave_partial_scaffolds() {
    let directory = TestDirectory::new("workspace-no-partial");

    let manifest_project = directory.path().join("desktop-manifest-exists");
    let desktop = manifest_project.join("apps/desktop-manifest-exists");
    fs::create_dir_all(&desktop).unwrap();
    fs::write(desktop.join("Cargo.toml"), "manifest sentinel").unwrap();
    let manifest_output =
        directory.run(&["init", "desktop-manifest-exists", "--layout", "workspace"]);
    assert!(!manifest_output.status.success());
    assert_eq!(
        fs::read_to_string(desktop.join("Cargo.toml")).unwrap(),
        "manifest sentinel"
    );
    assert!(!manifest_project.join("Cargo.toml").exists());
    assert!(!desktop.join("src/main.rs").exists());

    let blocked_project = directory.path().join("apps-is-a-file");
    fs::create_dir(&blocked_project).unwrap();
    fs::write(blocked_project.join("apps"), "apps sentinel").unwrap();
    let blocked_output = directory.run(&["init", "apps-is-a-file", "--layout", "workspace"]);
    assert!(!blocked_output.status.success());
    assert_eq!(
        fs::read_to_string(blocked_project.join("apps")).unwrap(),
        "apps sentinel"
    );
    assert!(!blocked_project.join("Cargo.toml").exists());
}

#[test]
fn failed_init_removes_a_new_target_directory() {
    let directory = TestDirectory::new("cleanup-new-target");

    let output = directory.run(&["init", "123-invalid", "--layout", "workspace"]);

    assert!(!output.status.success());
    assert!(!directory.path().join("123-invalid").exists());
}

#[test]
fn invalid_layout_is_rejected_before_creating_a_project() {
    let directory = TestDirectory::new("invalid-layout");

    let output = directory.run(&["create", "demo", "--layout", "nested"]);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid value 'nested'"));
    assert!(!directory.path().join("demo").exists());
}

#[test]
fn invalid_feature_is_rejected_before_creating_a_project() {
    let directory = TestDirectory::new("invalid-feature");

    let output = directory.run(&[
        "create",
        "demo",
        "--layout",
        "workspace",
        "--features",
        "unknown",
    ]);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid value 'unknown'"));
    assert!(!directory.path().join("demo").exists());
}

#[test]
fn create_refuses_an_existing_target_directory() {
    let directory = TestDirectory::new("existing-target");
    let project = directory.path().join("demo");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("marker.txt"), "keep me").unwrap();

    let output = directory.run(&["create", "demo", "--layout", "workspace"]);
    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(project.join("marker.txt")).unwrap(),
        "keep me"
    );
    assert!(!project.join("Cargo.toml").exists());
    assert!(!project.join("apps").exists());
}
