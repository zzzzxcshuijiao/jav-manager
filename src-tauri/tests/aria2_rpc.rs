use media_manager::aria2::{Aria2Client, Aria2RpcEndpoint, Aria2Transport};
use media_manager::pipeline::Aria2TaskSnapshot;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct RecordingTransport {
    response: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl RecordingTransport {
    fn new(response: &str) -> Self {
        Self {
            response: response.to_string(),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn last_request_json(&self) -> Value {
        let requests = self.requests.lock().unwrap();
        serde_json::from_str(requests.last().unwrap()).unwrap()
    }
}

impl Aria2Transport for RecordingTransport {
    fn post_json(
        &self,
        _endpoint: &Aria2RpcEndpoint,
        body: &str,
    ) -> anyhow::Result<String> {
        self.requests.lock().unwrap().push(body.to_string());
        Ok(self.response.clone())
    }
}

fn endpoint_with_secret() -> Aria2RpcEndpoint {
    Aria2RpcEndpoint {
        host: "127.0.0.1".to_string(),
        port: 6800,
        path: "/jsonrpc".to_string(),
        secret: Some("secret-value".to_string()),
        timeout_ms: 1_000,
    }
}

#[test]
fn tell_status_places_secret_token_before_gid() {
    let transport = RecordingTransport::new(
        r#"{"jsonrpc":"2.0","id":"media-manager-tell-status","result":{"gid":"abc","status":"active","totalLength":"10","completedLength":"4","files":[]}}"#,
    );
    let client = Aria2Client::new(endpoint_with_secret(), transport.clone());

    let status = client.tell_status("abc").unwrap();

    assert_eq!(status.gid, "abc");
    let request = transport.last_request_json();
    assert_eq!(request["method"], "aria2.tellStatus");
    assert_eq!(request["params"][0], "token:secret-value");
    assert_eq!(request["params"][1], "abc");
    assert_eq!(request["params"][2][0], "gid");
    assert_eq!(request["params"][2][4], "files");
}

#[test]
fn tell_status_parses_string_lengths_into_task_snapshot() {
    let transport = RecordingTransport::new(
        r#"{"jsonrpc":"2.0","id":"media-manager-tell-status","result":{"gid":"done","status":"complete","totalLength":"12","completedLength":"12","files":[]}}"#,
    );
    let client = Aria2Client::new(endpoint_with_secret(), transport);

    let status = client.tell_status("done").unwrap();

    assert_eq!(
        status.to_task_snapshot().unwrap(),
        Aria2TaskSnapshot {
            status: "complete".to_string(),
            completed_length: 12,
            total_length: 12,
        }
    );
    assert!(status.is_complete().unwrap());
}

#[test]
fn tell_status_returns_json_rpc_error_message() {
    let transport = RecordingTransport::new(
        r#"{"jsonrpc":"2.0","id":"media-manager-tell-status","error":{"code":1,"message":"Unauthorized"}}"#,
    );
    let client = Aria2Client::new(endpoint_with_secret(), transport);

    let error = client.tell_status("abc").unwrap_err();

    assert!(error.to_string().contains("aria2 JSON-RPC error 1"));
    assert!(error.to_string().contains("Unauthorized"));
}

#[test]
fn completed_selection_keeps_only_selected_completed_existing_videos() {
    let tmp = tempfile::tempdir().unwrap();
    let video = tmp.path().join("ABP-600.mp4");
    let unselected = tmp.path().join("ABP-601.mp4");
    let partial = tmp.path().join("ABP-602.mp4");
    let notes = tmp.path().join("notes.txt");
    std::fs::write(&video, b"good").unwrap();
    std::fs::write(&unselected, b"skip").unwrap();
    std::fs::write(&partial, b"half").unwrap();
    std::fs::write(&notes, b"note").unwrap();
    let response = json!({
        "jsonrpc": "2.0",
        "id": "media-manager-tell-status",
        "result": {
            "gid": "done",
            "status": "complete",
            "totalLength": "20",
            "completedLength": "20",
            "files": [
                {"path": video.to_string_lossy().to_string(), "length": "4", "completedLength": "4", "selected": "true"},
                {"path": unselected.to_string_lossy().to_string(), "length": "4", "completedLength": "4", "selected": "false"},
                {"path": partial.to_string_lossy().to_string(), "length": "4", "completedLength": "2", "selected": "true"},
                {"path": notes.to_string_lossy().to_string(), "length": "4", "completedLength": "4", "selected": "true"},
                {"path": tmp.path().join("missing.mp4").to_string_lossy().to_string(), "length": "4", "completedLength": "4", "selected": "true"}
            ]
        }
    })
    .to_string();
    let client = Aria2Client::new(endpoint_with_secret(), RecordingTransport::new(&response));

    let status = client.tell_status("done").unwrap();
    let selection = status.completed_selection().unwrap();

    assert_eq!(selection.scanned_files, 5);
    assert_eq!(selection.skipped_files, 4);
    assert_eq!(selection.files.len(), 1);
    assert_eq!(selection.files[0].path, video);
    assert_eq!(selection.files[0].file_name, "ABP-600.mp4");
}

#[test]
fn completed_selection_is_empty_for_unfinished_task() {
    let response = r#"{"jsonrpc":"2.0","id":"media-manager-tell-status","result":{"gid":"active","status":"active","totalLength":"20","completedLength":"20","files":[]}}"#;
    let client = Aria2Client::new(endpoint_with_secret(), RecordingTransport::new(response));

    let status = client.tell_status("active").unwrap();
    let selection = status.completed_selection().unwrap();

    assert_eq!(selection.scanned_files, 0);
    assert_eq!(selection.skipped_files, 0);
    assert!(selection.files.is_empty());
}
