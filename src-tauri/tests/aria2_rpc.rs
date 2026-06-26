use media_manager::aria2::{Aria2Client, Aria2RpcEndpoint, Aria2Transport};
use media_manager::pipeline::Aria2TaskSnapshot;
use serde_json::Value;
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
