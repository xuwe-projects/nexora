use std::{
    io::{Read as _, Write as _},
    net::TcpListener,
    thread,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer as _, SigningKey};
use semver::Version;
use sha2::{Digest as _, Sha256};
use updater::{
    ReleaseStatus, SignedUpdateManifest, UpdateArtifact, UpdateChannel, UpdateConfig, UpdateEvent,
    UpdateManifest, UpdateManifestSignature, UpdateTarget, Updater,
};

struct NotesFixture {
    updater: Updater,
    release: updater::UpdateRelease,
    server: thread::JoinHandle<()>,
}

fn notes_fixture(
    body: Vec<u8>,
    notes_sha256: Option<String>,
    notes_size: Option<u64>,
) -> NotesFixture {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let signing_key = SigningKey::from_bytes(&[41_u8; 32]);
    let target = UpdateTarget::current().unwrap();
    let payload = UpdateManifest {
        manifest_sequence: 2,
        app_id: "com.example.release-notes".to_owned(),
        channel: UpdateChannel::Stable,
        version: Version::parse("1.1.0").unwrap(),
        build_number: 2,
        minimum_supported_version: Version::parse("1.0.0").unwrap(),
        published_at: 1_784_304_000,
        status: ReleaseStatus::Available,
        notes_url: Some("notes.md".to_owned()),
        notes_sha256,
        notes_size,
        artifacts: vec![UpdateArtifact {
            target: target.as_str().to_owned(),
            url: "update.bin".to_owned(),
            sha256: "00".repeat(32),
            size: 1,
            kind: "test".to_owned(),
        }],
    };
    let signature = signing_key.sign(&serde_json::to_vec(&payload).unwrap());
    let manifest = serde_json::to_vec(&SignedUpdateManifest {
        schema_version: 1,
        payload,
        signatures: vec![UpdateManifestSignature {
            key_id: "notes-test".to_owned(),
            algorithm: "ed25519".to_owned(),
            signature: STANDARD.encode(signature.to_bytes()),
        }],
    })
    .unwrap();
    let request_count = if manifest
        .windows(b"notes_sha256".len())
        .any(|window| window == b"notes_sha256")
    {
        2
    } else {
        1
    };
    let server = thread::spawn(move || {
        for _ in 0..request_count {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2048];
            let read = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..read]);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap();
            let response = if path == "/latest.json" {
                manifest.as_slice()
            } else {
                assert_eq!(path, "/notes.md");
                body.as_slice()
            };
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                response.len()
            )
            .unwrap();
            stream.write_all(response).unwrap();
        }
    });
    let trusted_key = format!(
        "notes-test:ed25519:{}",
        STANDARD.encode(signing_key.verifying_key().to_bytes())
    );
    let config = UpdateConfig::new(
        format!("http://{address}/latest.json"),
        "com.example.release-notes",
        "1.0.0",
        1,
        UpdateChannel::Stable,
    )
    .unwrap()
    .with_trusted_public_keys([trusted_key])
    .unwrap();
    let updater = Updater::new(config);
    let session = updater.check().unwrap();
    let events = session.events();
    assert!(matches!(
        events.recv_blocking().unwrap(),
        UpdateEvent::Checking
    ));
    let UpdateEvent::UpdateAvailable(release) = events.recv_blocking().unwrap() else {
        panic!("签名清单应返回可用更新");
    };
    NotesFixture {
        updater,
        release,
        server,
    }
}

#[test]
fn downloads_only_fully_described_trusted_release_notes() {
    let body = b"# Changes\n\n- Verified notes.\n".to_vec();
    let fixture = notes_fixture(
        body.clone(),
        Some(format!("{:x}", Sha256::digest(&body))),
        Some(body.len() as u64),
    );

    let markdown = fixture
        .updater
        .fetch_release_notes(&fixture.release)
        .unwrap()
        .unwrap();
    assert!(markdown.contains("Verified notes"));
    fixture.server.join().unwrap();
}

#[test]
fn legacy_release_notes_url_is_not_downloaded_without_integrity_fields() {
    let fixture = notes_fixture(b"untrusted".to_vec(), None, None);

    assert!(
        fixture
            .updater
            .fetch_release_notes(&fixture.release)
            .unwrap()
            .is_none()
    );
    fixture.server.join().unwrap();
}

#[test]
fn rejects_release_notes_size_checksum_and_utf8_failures() {
    let valid = b"# Valid\n".to_vec();
    let cases = [
        (
            valid.clone(),
            format!("{:x}", Sha256::digest(&valid)),
            valid.len() as u64 - 1,
        ),
        (valid.clone(), "00".repeat(32), valid.len() as u64),
        (
            vec![0xff, 0xfe],
            format!("{:x}", Sha256::digest([0xff, 0xfe])),
            2,
        ),
    ];

    for (body, checksum, size) in cases {
        let fixture = notes_fixture(body, Some(checksum), Some(size));
        assert!(
            fixture
                .updater
                .fetch_release_notes(&fixture.release)
                .is_err()
        );
        fixture.server.join().unwrap();
    }
}
