use std::{
    io::{Read as _, Write as _},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::{SystemTime, UNIX_EPOCH},
};

use account::authentication::{
    AccessTokenVerifier, VerificationError, ZitadelIntrospectionVerifier,
};
use serde_json::json;

const AUDIENCE: &str = "nexora-api";

#[tokio::test]
async fn active_pat_is_introspected_for_every_request() {
    let provider = IntrospectionProvider::spawn(2, |issuer| {
        json!({
            "active": true,
            "sub": "machine-1",
            "iss": issuer,
            "aud": ["other-api", AUDIENCE],
            "exp": now() + 3600,
            "username": "dispenser-line-a",
            "name": "A 线点料机"
        })
        .to_string()
    });
    let verifier = verifier(&provider.issuer);

    for _ in 0..2 {
        let identity = verifier
            .verify("opaque-pat-value")
            .await
            .expect("有效 PAT 应当通过 introspection");
        assert_eq!(identity.subject, "machine-1");
        assert_eq!(identity.username.as_deref(), Some("dispenser-line-a"));
    }

    assert_eq!(provider.requests(), 2, "成功结果不得缓存");
    provider.join();
}

#[tokio::test]
async fn inactive_or_mismatched_pat_is_rejected() {
    for body in [
        json!({ "active": false }).to_string(),
        json!({
            "active": true,
            "sub": "machine-1",
            "iss": "https://wrong.example.com",
            "aud": AUDIENCE,
            "exp": now() + 3600
        })
        .to_string(),
    ] {
        let provider = IntrospectionProvider::spawn_with_body(body);
        let error = verifier(&provider.issuer)
            .verify("opaque-pat-value")
            .await
            .expect_err("inactive 或 issuer 不匹配的 PAT 必须被拒绝");
        assert!(matches!(error, VerificationError::InvalidToken));
        provider.join();
    }

    let provider = IntrospectionProvider::spawn(1, |issuer| {
        json!({
            "active": true,
            "sub": "machine-1",
            "iss": issuer,
            "aud": "wrong-api",
            "exp": now() + 3600
        })
        .to_string()
    });
    let error = verifier(&provider.issuer)
        .verify("opaque-pat-value")
        .await
        .expect_err("audience 不匹配的 PAT 必须被拒绝");
    assert!(matches!(error, VerificationError::InvalidToken));
    provider.join();
}

#[tokio::test]
async fn unavailable_introspection_fails_closed_without_exposing_secrets() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("应当可以分配测试端口");
    let issuer = format!(
        "http://{}",
        listener.local_addr().expect("应当可以读取测试端口")
    );
    drop(listener);
    let verifier = verifier(&issuer);
    let debug = format!("{verifier:?}");
    assert!(!debug.contains("resource-server-secret"));
    assert!(debug.contains("<redacted>"));

    let error = verifier
        .verify("opaque-pat-value")
        .await
        .expect_err("Provider 不可用时必须失败关闭");
    assert!(matches!(
        error,
        VerificationError::IntrospectionUnavailable(_)
    ));
    assert!(!format!("{error:?}").contains("opaque-pat-value"));
    assert!(!format!("{error:?}").contains("resource-server-secret"));
}

fn verifier(issuer: &str) -> ZitadelIntrospectionVerifier {
    ZitadelIntrospectionVerifier::new(
        issuer,
        AUDIENCE,
        "resource-server-client",
        "resource-server-secret",
    )
    .expect("loopback introspection verifier 应当可以创建")
}

struct IntrospectionProvider {
    issuer: String,
    requests: Arc<AtomicUsize>,
    server: Option<JoinHandle<()>>,
}

impl IntrospectionProvider {
    fn spawn(expected_requests: usize, body: impl Fn(&str) -> String + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("应当可以绑定测试 Provider");
        let issuer = format!(
            "http://{}",
            listener.local_addr().expect("应当可以读取监听地址")
        );
        let server_issuer = issuer.clone();
        let requests = Arc::new(AtomicUsize::new(0));
        let server_requests = Arc::clone(&requests);
        let server = thread::spawn(move || {
            for _ in 0..expected_requests {
                let (mut stream, _) = listener.accept().expect("应当接收 introspection 请求");
                let request = read_request(&mut stream);
                assert!(request.starts_with("POST /oauth/v2/introspect HTTP/1.1"));
                assert!(
                    request
                        .to_ascii_lowercase()
                        .contains("authorization: basic ")
                );
                assert!(request.ends_with("token=opaque-pat-value"));
                server_requests.fetch_add(1, Ordering::AcqRel);
                write_response(&mut stream, body(server_issuer.as_str()).as_str());
            }
        });
        Self {
            issuer,
            requests,
            server: Some(server),
        }
    }

    fn spawn_with_body(body: String) -> Self {
        Self::spawn(1, move |_| body.clone())
    }

    fn requests(&self) -> usize {
        self.requests.load(Ordering::Acquire)
    }

    fn join(mut self) {
        self.server
            .take()
            .expect("测试线程应存在")
            .join()
            .expect("测试 Provider 应当正常退出");
    }
}

fn read_request(stream: &mut std::net::TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let count = stream.read(&mut buffer).expect("应当读取 HTTP 请求");
        assert!(count > 0, "HTTP 请求不应提前关闭");
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            let header_end = header_end + 4;
            let headers = String::from_utf8_lossy(&bytes[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|value| value.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if bytes.len() >= header_end + content_length {
                return String::from_utf8(bytes).expect("测试 HTTP 请求应为 UTF-8");
            }
        }
    }
}

fn write_response(stream: &mut std::net::TcpStream, body: &str) {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .expect("应当写入 introspection 响应");
}

fn now() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("系统时间应晚于 Unix epoch")
            .as_secs(),
    )
    .expect("当前 Unix 秒应适合 i64")
}
