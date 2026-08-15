use std::{env, error::Error, ffi::OsString, path::Path};

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::oneshot;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::header},
};
use workmand::{DaemonConfig, DaemonServer};

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

struct ConfigGuard(Option<OsString>);

impl ConfigGuard {
    fn set(path: &Path) -> Self {
        let previous = env::var_os("WORKMAN_CONFIG");
        // SAFETY: this integration binary has one test and restores the value after shutdown.
        unsafe { env::set_var("WORKMAN_CONFIG", path) };
        Self(previous)
    }
}

impl Drop for ConfigGuard {
    fn drop(&mut self) {
        // SAFETY: no sibling test in this integration process reads this value.
        unsafe {
            match self.0.take() {
                Some(value) => env::set_var("WORKMAN_CONFIG", value),
                None => env::remove_var("WORKMAN_CONFIG"),
            }
        }
    }
}

async fn rpc(socket: &mut Socket, id: u64, method: &str, params: Value) -> Value {
    socket
        .send(Message::Text(
            json!({ "id": id, "method": method, "params": params })
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
        panic!("expected a JSON control response");
    };
    let response: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(response["id"], id);
    response
}

#[tokio::test]
async fn websocket_round_trips_quick_prompt_crud_and_order() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let _config = ConfigGuard::set(&temp.path().join("config.yml"));
    let server = DaemonServer::bind(DaemonConfig {
        data_dir: temp.path().join("state"),
        port: 0,
    })
    .await?;
    let discovery = server.discovery().clone();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server_task = tokio::spawn(server.serve_until(async move {
        let _ = shutdown_rx.await;
    }));

    let mut request = format!("ws://127.0.0.1:{}/ws", discovery.port).into_client_request()?;
    request.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {}", discovery.token).parse()?,
    );
    let (mut socket, _) = connect_async(request).await?;

    let empty = rpc(&mut socket, 1, "quick_prompts.list", json!({})).await;
    assert_eq!(empty["result"], json!([]));

    let first = rpc(
        &mut socket,
        2,
        "quick_prompts.save",
        json!({ "prompt": { "name": "Review", "body": "one\ntwo\nthree" } }),
    )
    .await;
    assert_eq!(first["ok"], true);
    assert_eq!(first["result"]["name"], "Review");
    assert_eq!(first["result"]["body"], "one\ntwo\nthree");
    let first_id = first["result"]["id"].as_i64().unwrap();

    let second = rpc(
        &mut socket,
        3,
        "quick_prompts.save",
        json!({ "prompt": { "name": "Summarize", "body": "Summarize this." } }),
    )
    .await;
    let second_id = second["result"]["id"].as_i64().unwrap();

    let updated = rpc(
        &mut socket,
        4,
        "quick_prompts.save",
        json!({
            "prompt": { "id": first_id, "name": "Review carefully", "body": "Find regressions." }
        }),
    )
    .await;
    assert_eq!(updated["result"]["body"], "Find regressions.");

    let reordered = rpc(
        &mut socket,
        5,
        "quick_prompts.reorder",
        json!({ "quick_prompt_ids": [second_id, first_id] }),
    )
    .await;
    assert_eq!(reordered["result"][0]["id"], second_id);
    assert_eq!(reordered["result"][1]["id"], first_id);

    let deleted = rpc(
        &mut socket,
        6,
        "quick_prompts.delete",
        json!({ "quick_prompt_id": first_id }),
    )
    .await;
    assert_eq!(deleted["result"]["deleted"], true);
    let listed = rpc(&mut socket, 7, "quick_prompts.list", json!({})).await;
    assert_eq!(listed["result"].as_array().unwrap().len(), 1);
    assert_eq!(listed["result"][0]["id"], second_id);

    socket.close(None).await?;
    let _ = shutdown_tx.send(());
    server_task.await??;
    Ok(())
}
