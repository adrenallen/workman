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

async fn rpc(socket: &mut Socket, id: u64, method: &str, params: Value) -> Value {
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
            assert_eq!(response["ok"], true, "RPC failed: {response}");
            return response["result"].clone();
        }
    }
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
async fn folder_layout_and_collapse_survive_daemon_restart_and_delete_lifts_children() {
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("com.workman.todo85-state");
    let mut project_ids = Vec::new();
    for name in ["alpha", "beta", "gamma"] {
        let path = temp.path().join(name);
        fs::create_dir(&path).unwrap();
        project_ids.push(path);
    }

    let server = DaemonServer::bind(DaemonConfig {
        data_dir: state.clone(),
        port: 0,
    })
    .await
    .unwrap();
    let discovery = server.discovery().clone();
    let task = tokio::spawn(server.serve_until(pending()));
    let (mut socket, _) = connect_async(request(&discovery)).await.unwrap();

    let mut ids = Vec::new();
    for (offset, path) in project_ids.iter().enumerate() {
        let projects = rpc(
            &mut socket,
            offset as u64 + 1,
            "projects.register",
            json!({ "path": workman_core::canonical_path(path).unwrap() }),
        )
        .await;
        ids = projects
            .as_array()
            .unwrap()
            .iter()
            .map(|project| project["id"].as_i64().unwrap())
            .collect();
    }
    let later = rpc(
        &mut socket,
        4,
        "project_folders.create",
        json!({ "name": "Later" }),
    )
    .await["folders"][0]["id"]
        .as_i64()
        .unwrap();
    let review = rpc(
        &mut socket,
        5,
        "project_folders.create",
        json!({ "name": "Review" }),
    )
    .await["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|folder| folder["name"] == "Review")
        .unwrap()["id"]
        .as_i64()
        .unwrap();

    rpc(
        &mut socket,
        6,
        "project.layout",
        json!({
            "entries": [
                { "kind": "folder", "id": review, "project_ids": [ids[2]] },
                { "kind": "project", "id": ids[0] },
                { "kind": "folder", "id": later, "project_ids": [ids[1]] }
            ]
        }),
    )
    .await;
    rpc(
        &mut socket,
        7,
        "project_folders.set_collapsed",
        json!({ "folder_id": later, "collapsed": true }),
    )
    .await;
    rpc(
        &mut socket,
        8,
        "project_folders.update_settings",
        json!({
            "folder_id": later,
            "name": "Later",
            "icon": "boxes",
            "name_color": "violet"
        }),
    )
    .await;
    socket.close(None).await.unwrap();
    task.abort();
    let _ = task.await;

    let server = DaemonServer::bind(DaemonConfig {
        data_dir: state,
        port: 0,
    })
    .await
    .unwrap();
    let discovery = server.discovery().clone();
    let task = tokio::spawn(server.serve_until(pending()));
    let (mut socket, _) = connect_async(request(&discovery)).await.unwrap();
    let restored = rpc(&mut socket, 8, "project.rail", json!({})).await;
    assert_eq!(restored["layout"][0]["id"], review);
    assert_eq!(restored["layout"][1]["id"], ids[0]);
    assert_eq!(restored["layout"][2]["id"], later);
    assert_eq!(restored["layout"][2]["project_ids"], json!([ids[1]]));
    assert_eq!(
        restored["folders"]
            .as_array()
            .unwrap()
            .iter()
            .find(|folder| folder["id"] == later)
            .unwrap()["collapsed"],
        true
    );
    let restored_later = restored["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|folder| folder["id"] == later)
        .unwrap();
    assert_eq!(restored_later["icon"], "boxes");
    assert_eq!(restored_later["name_color"], "violet");

    let lifted = rpc(
        &mut socket,
        9,
        "project_folders.delete",
        json!({ "folder_id": review, "confirm_delete": true }),
    )
    .await;
    assert_eq!(lifted["projects"].as_array().unwrap().len(), 3);
    assert_eq!(
        lifted["layout"][0],
        json!({ "kind": "project", "id": ids[2] })
    );
    assert!(
        lifted["folders"]
            .as_array()
            .unwrap()
            .iter()
            .all(|folder| folder["id"] != review)
    );

    let profiles = rpc(&mut socket, 10, "profile.list", json!({})).await;
    let default_profile_id = profiles["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|profile| profile["active"] == true)
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    let archive = temp.path().join("folder-layout.workman-profile.json");
    rpc(
        &mut socket,
        11,
        "profile.export",
        json!({ "profile_id": default_profile_id, "path": archive }),
    )
    .await;
    let imported = rpc(
        &mut socket,
        12,
        "profile.import",
        json!({ "path": archive, "name": "Imported folders" }),
    )
    .await;
    rpc(
        &mut socket,
        13,
        "profile.switch",
        json!({ "profile_id": imported["profile"]["id"] }),
    )
    .await;
    let imported_rail = rpc(&mut socket, 14, "project.rail", json!({})).await;
    assert_eq!(imported_rail["folders"].as_array().unwrap().len(), 1);
    assert_eq!(imported_rail["folders"][0]["name"], "Later");
    assert_eq!(imported_rail["folders"][0]["collapsed"], true);
    assert_eq!(imported_rail["folders"][0]["icon"], "boxes");
    assert_eq!(imported_rail["folders"][0]["name_color"], "violet");
    assert_eq!(imported_rail["layout"][2]["kind"], "folder");
    assert_eq!(imported_rail["layout"][2]["project_ids"], json!([ids[1]]));

    socket.close(None).await.unwrap();
    task.abort();
}
