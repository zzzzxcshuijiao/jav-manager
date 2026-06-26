use media_manager::diagnostics::{
    redact_diagnostic_value, redact_proxy_url, DiagnosticLevel, DiagnosticsWriter,
};
use serde_json::json;

#[test]
fn diagnostics_writer_appends_jsonl_and_reads_tail() {
    let tmp = tempfile::tempdir().unwrap();
    let writer =
        DiagnosticsWriter::new_with_limits(tmp.path().join("logs"), 64 * 1024, 2).unwrap();

    writer
        .append(
            DiagnosticLevel::Info,
            "daemon.run_once",
            "run started",
            json!({ "source_roots": 2 }),
        )
        .unwrap();
    writer
        .append(
            DiagnosticLevel::Warn,
            "settings.aria2",
            "secret configured",
            json!({ "secret": "plain-secret", "tracked_gids": 1 }),
        )
        .unwrap();

    let tail = writer.tail(10).unwrap();

    assert_eq!(tail.len(), 2);
    assert_eq!(tail[0].target, "daemon.run_once");
    assert_eq!(tail[0].message, "run started");
    assert_eq!(tail[1].level, DiagnosticLevel::Warn);
    assert_eq!(tail[1].context["secret"], "***");
    assert!(writer.log_path().ends_with("media-manager.jsonl"));
}

#[test]
fn diagnostics_tail_limit_is_bounded_and_ordered_oldest_to_newest() {
    let tmp = tempfile::tempdir().unwrap();
    let writer =
        DiagnosticsWriter::new_with_limits(tmp.path().join("logs"), 64 * 1024, 2).unwrap();

    for index in 0..120 {
        writer
            .append(
                DiagnosticLevel::Info,
                "test.sequence",
                format!("entry {index}"),
                json!({ "index": index }),
            )
            .unwrap();
    }

    let tail = writer.tail(3).unwrap();

    assert_eq!(tail.len(), 3);
    assert_eq!(tail[0].message, "entry 117");
    assert_eq!(tail[2].message, "entry 119");

    let bounded = writer.tail(usize::MAX).unwrap();
    assert!(bounded.len() <= 200);
}

#[test]
fn diagnostics_writer_rotates_when_file_exceeds_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let writer = DiagnosticsWriter::new_with_limits(tmp.path().join("logs"), 180, 2).unwrap();

    for index in 0..8 {
        writer
            .append(
                DiagnosticLevel::Info,
                "test.rotation",
                format!("entry {index}"),
                json!({ "padding": "abcdefghijklmnopqrstuvwxyz" }),
            )
            .unwrap();
    }

    assert!(writer.log_path().exists());
    assert!(tmp.path().join("logs").join("media-manager.jsonl.1").exists());
    assert!(!tmp.path().join("logs").join("media-manager.jsonl.3").exists());
}

#[test]
fn diagnostic_redaction_covers_secrets_tokens_cookies_and_proxy_credentials() {
    let redacted = redact_diagnostic_value(json!({
        "secret": "abc",
        "access_token": "token",
        "headers": {
            "Authorization": "Bearer abc",
            "Cookie": "session=abc",
            "safe": "kept"
        },
        "nested": [{ "password": "pw" }]
    }));

    assert_eq!(redacted["secret"], "***");
    assert_eq!(redacted["access_token"], "***");
    assert_eq!(redacted["headers"]["Authorization"], "***");
    assert_eq!(redacted["headers"]["Cookie"], "***");
    assert_eq!(redacted["headers"]["safe"], "kept");
    assert_eq!(redacted["nested"][0]["password"], "***");
    assert_eq!(
        redact_proxy_url("http://user:pass@127.0.0.1:8080/proxy"),
        "http://***@127.0.0.1:8080/proxy"
    );
}
