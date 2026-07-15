use std::{
    io::{BufRead as _, BufReader, Read as _, Write as _},
    net::{TcpListener, TcpStream},
    thread::{self, JoinHandle},
};

use account::directory::ZitadelUserDirectory;
use serde_json::{Value, json};

const TEST_TOKEN: &str = "test-bootstrap-pat";

#[test]
fn directory_requires_https_except_for_loopback_development() {
    assert!(ZitadelUserDirectory::new("http://id.example.com", TEST_TOKEN).is_err());
    assert!(ZitadelUserDirectory::new("https://id.example.com", TEST_TOKEN).is_ok());
    assert!(ZitadelUserDirectory::new("http://localhost:8080", TEST_TOKEN).is_ok());
}

#[tokio::test]
async fn directory_paginates_and_returns_only_active_human_users() {
    let (issuer, server) = spawn_zitadel();
    let directory = ZitadelUserDirectory::new(&issuer, TEST_TOKEN)
        .expect("有效 issuer 和 PAT 应当可以创建目录客户端");

    let users = directory
        .list_active_human_users()
        .await
        .expect("ZITADEL 用户目录应当读取成功");

    assert_eq!(users.len(), 2);
    assert_eq!(users[0].subject, "human-1");
    assert_eq!(users[0].display_name, "Ada Lovelace");
    assert_eq!(users[0].email.as_deref(), Some("ada@example.com"));
    assert_eq!(
        users[0].avatar_url.as_deref(),
        Some("https://example.com/ada.png")
    );
    assert_eq!(users[1].subject, "human-2");
    assert_eq!(users[1].display_name, "bob@example.com");
    server.join().expect("测试 ZITADEL 服务应当正常退出");
}

#[tokio::test]
async fn directory_queries_explicit_subject_without_reading_full_directory() {
    let (issuer, server) = spawn_subject_lookup();
    let directory = ZitadelUserDirectory::new(&issuer, TEST_TOKEN)
        .expect("有效 issuer 和 PAT 应当可以创建目录客户端");

    let user = directory
        .active_human_user("human-42")
        .await
        .expect("显式 subject 查询应当成功")
        .expect("指定用户应当存在");

    assert_eq!(user.subject, "human-42");
    assert_eq!(user.display_name, "Grace Hopper");
    server.join().expect("测试 ZITADEL 服务应当正常退出");
}

fn spawn_zitadel() -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("应当可以绑定测试 ZITADEL 服务");
    let issuer = format!(
        "http://{}",
        listener.local_addr().expect("应当可以读取监听地址")
    );
    let server = thread::spawn(move || {
        for page in 0..2 {
            let (mut stream, _) = listener.accept().expect("应当接收目录请求");
            let request = read_request(&mut stream);
            assert_eq!(request.path, "/v2/users");
            assert_eq!(
                request.authorization.as_deref(),
                Some("Bearer test-bootstrap-pat")
            );
            assert_eq!(
                request.body["sortingColumn"],
                "USER_FIELD_NAME_DISPLAY_NAME"
            );
            assert_eq!(request.body["query"]["limit"], 100);
            assert_eq!(request.body["query"]["asc"], true);
            assert_eq!(request.body["query"]["offset"], (page * 2).to_string());
            assert_eq!(
                request.body["queries"][0]["stateQuery"]["state"],
                "USER_STATE_ACTIVE"
            );
            assert_eq!(
                request.body["queries"][1]["typeQuery"]["type"],
                "TYPE_HUMAN"
            );

            let body = if page == 0 {
                json!({
                    "details": { "totalResult": "4" },
                    "result": [
                        {
                            "userId": "human-1",
                            "state": "USER_STATE_ACTIVE",
                            "username": "ada",
                            "preferredLoginName": "ada@example.com",
                            "human": {
                                "profile": {
                                    "displayName": "Ada Lovelace",
                                    "avatarUrl": "https://example.com/ada.png"
                                },
                                "email": { "email": "ada@example.com" }
                            }
                        },
                        {
                            "userId": "machine-1",
                            "state": "USER_STATE_ACTIVE",
                            "username": "worker",
                            "machine": { "name": "worker" }
                        }
                    ]
                })
            } else {
                json!({
                    "details": { "totalResult": 4 },
                    "result": [
                        {
                            "userId": "inactive-1",
                            "state": "USER_STATE_INACTIVE",
                            "username": "inactive",
                            "human": { "profile": { "displayName": "Inactive" } }
                        },
                        {
                            "userId": "human-2",
                            "state": "USER_STATE_ACTIVE",
                            "username": "bob",
                            "preferredLoginName": "bob@example.com",
                            "human": { "profile": { "displayName": "" } }
                        }
                    ]
                })
            };
            write_response(&mut stream, &body.to_string());
        }
    });
    (issuer, server)
}

fn spawn_subject_lookup() -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("应当可以绑定测试 ZITADEL 服务");
    let issuer = format!(
        "http://{}",
        listener.local_addr().expect("应当可以读取监听地址")
    );
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("应当接收目录请求");
        let request = read_request(&mut stream);
        assert_eq!(request.path, "/v2/users");
        assert_eq!(
            request.body["queries"][2]["inUserIdsQuery"]["userIds"][0],
            "human-42"
        );
        write_response(
            &mut stream,
            &json!({
                "details": { "totalResult": "1" },
                "result": [{
                    "userId": "human-42",
                    "state": "USER_STATE_ACTIVE",
                    "username": "grace",
                    "human": { "profile": { "displayName": "Grace Hopper" } }
                }]
            })
            .to_string(),
        );
    });
    (issuer, server)
}

struct Request {
    path: String,
    authorization: Option<String>,
    body: Value,
}

fn read_request(stream: &mut TcpStream) -> Request {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).expect("应当读取请求行");
    let path = request_line
        .split_whitespace()
        .nth(1)
        .expect("请求行应当包含路径")
        .to_owned();
    let mut content_length = 0;
    let mut authorization = None;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).expect("应当读取请求头");
        if header == "\r\n" || header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().expect("正文长度应当有效");
            } else if name.eq_ignore_ascii_case("authorization") {
                authorization = Some(value.trim().to_owned());
            }
        }
    }
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body).expect("应当读取请求正文");
    Request {
        path,
        authorization,
        body: serde_json::from_slice(&body).expect("请求正文应当是 JSON"),
    }
}

fn write_response(stream: &mut TcpStream, body: &str) {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .expect("应当写入目录响应");
}
