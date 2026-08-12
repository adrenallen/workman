#![cfg(unix)]

use std::{fs, future::pending, time::Duration};

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::header},
};
use workmand::{DaemonConfig, DaemonServer, Discovery};

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

async fn rpc_response(socket: &mut Socket, id: u64, method: &str, params: Value) -> Value {
    socket
        .send(Message::Text(
            json!({ "id": id, "method": method, "params": params })
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    loop {
        let message = tokio::time::timeout(Duration::from_secs(10), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let Message::Text(message) = message else {
            continue;
        };
        let response: Value = serde_json::from_str(&message).unwrap();
        if response["id"] == id {
            return response;
        }
    }
}

async fn rpc(socket: &mut Socket, id: u64, method: &str, params: Value) -> Value {
    let response = rpc_response(socket, id, method, params).await;
    assert_eq!(response["ok"], true, "RPC failed: {response}");
    response["result"].clone()
}

fn request(discovery: &Discovery) -> axum::http::Request<()> {
    let mut request = format!("ws://127.0.0.1:{}/ws", discovery.port)
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {}", discovery.token).parse().unwrap(),
    );
    request
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn profile_switch_confirmation_keeps_endpoint_and_round_trips_archive() {
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("com.workman.todo84-state");
    let original = temp.path().join("original");
    let throwaway = temp.path().join("throwaway");
    fs::create_dir(&original).unwrap();
    fs::create_dir(&throwaway).unwrap();

    let server = DaemonServer::bind(DaemonConfig {
        data_dir: state.clone(),
        port: 0,
    })
    .await
    .unwrap();
    let discovery = server.discovery().clone();
    let task = tokio::spawn(server.serve_until(pending()));
    let (mut socket, _) = connect_async(request(&discovery)).await.unwrap();

    let projects = rpc(
        &mut socket,
        1,
        "projects.register",
        json!({ "path": fs::canonicalize(&original).unwrap() }),
    )
    .await;
    let original_id = projects[0]["id"].as_i64().unwrap();
    let terminal = rpc(
        &mut socket,
        2,
        "process.spawn_terminal",
        json!({ "project_id": original_id, "name": "profile switch guard" }),
    )
    .await;
    let process_id = terminal["id"].as_i64().unwrap();
    assert_eq!(terminal["status"], "running");

    let created = rpc(
        &mut socket,
        3,
        "profile.create",
        json!({ "name": "Recording", "copy_current": false }),
    )
    .await;
    let recording_id = created["profile"]["id"].as_i64().unwrap();
    let rejected = rpc_response(
        &mut socket,
        4,
        "profile.switch",
        json!({ "profile_id": recording_id }),
    )
    .await;
    assert_eq!(rejected["ok"], false);
    assert_eq!(
        rejected["error"]["code"],
        "profile_switch_requires_confirmation"
    );
    let switched = rpc(
        &mut socket,
        5,
        "profile.switch",
        json!({ "profile_id": recording_id, "confirm_stop_running": true }),
    )
    .await;
    assert_eq!(switched["profile"]["name"], "Recording");
    assert_eq!(switched["stopped_processes"], json!([process_id]));
    assert_eq!(
        rpc(&mut socket, 6, "projects.list", json!({})).await,
        json!([])
    );

    let throwaway_projects = rpc(
        &mut socket,
        7,
        "projects.register",
        json!({ "path": fs::canonicalize(&throwaway).unwrap() }),
    )
    .await;
    assert_eq!(
        throwaway_projects[0]["path"],
        fs::canonicalize(&throwaway)
            .unwrap()
            .to_string_lossy()
            .as_ref()
    );
    let profiles = rpc(&mut socket, 8, "profile.list", json!({})).await;
    let default_id = profiles["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|profile| profile["name"] == "Default")
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    rpc(
        &mut socket,
        9,
        "profile.switch",
        json!({ "profile_id": default_id }),
    )
    .await;
    let restored = rpc(&mut socket, 10, "projects.list", json!({})).await;
    assert_eq!(restored.as_array().unwrap().len(), 1);
    assert_eq!(restored[0]["id"], original_id);

    let archive = temp.path().join("default.workman-profile.json");
    rpc(
        &mut socket,
        11,
        "profile.export",
        json!({ "profile_id": default_id, "path": archive }),
    )
    .await;
    let archive_text = fs::read_to_string(&archive).unwrap();
    assert!(!archive_text.contains(&discovery.token));
    for forbidden in ["download_key", "signing", "process_env", "mcp-endpoint"] {
        assert!(!archive_text.contains(forbidden));
    }
    let imported = rpc(
        &mut socket,
        12,
        "profile.import",
        json!({ "path": archive, "name": "Round trip" }),
    )
    .await;
    let imported_id = imported["profile"]["id"].as_i64().unwrap();
    rpc(
        &mut socket,
        13,
        "profile.switch",
        json!({ "profile_id": imported_id }),
    )
    .await;
    let imported_projects = rpc(&mut socket, 14, "projects.list", json!({})).await;
    assert_eq!(imported_projects.as_array().unwrap().len(), 1);
    assert_eq!(imported_projects[0]["id"], original_id);

    // The original authenticated socket and persistent endpoint remain valid through switches.
    let discovery_after = Discovery::read(&state).unwrap();
    assert_eq!(discovery_after.port, discovery.port);
    assert_eq!(discovery_after.token, discovery.token);
    assert_eq!(
        rpc(&mut socket, 15, "profile.list", json!({})).await["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|profile| profile["active"] == true)
            .count(),
        1
    );

    socket.close(None).await.unwrap();
    task.abort();
}
