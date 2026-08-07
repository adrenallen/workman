use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::{sync::oneshot, task::JoinHandle};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::header},
};
use workman_core::{NewTodo, Project, ScratchpadService, TodoPriority, TodoService};
use workmand::{DaemonConfig, DaemonServer, Discovery, SharedProcessRegistry};

struct TestServer {
    discovery: Discovery,
    registry: SharedProcessRegistry,
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<std::io::Result<()>>,
    _temp: TempDir,
    _project_path: PathBuf,
}

impl TestServer {
    async fn start() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let project_path = temp.path().join("project");
        std::fs::create_dir(&project_path).unwrap();
        let server = DaemonServer::bind(DaemonConfig {
            data_dir: temp.path().join("state"),
            port: 0,
        })
        .await
        .unwrap();
        let discovery = server.discovery().clone();
        let registry = server.registry();
        {
            let registry = registry.lock().await;
            let store = registry.store();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            store
                .put_project(&Project {
                    id: 1,
                    path: project_path.to_string_lossy().into_owned(),
                    name: "project".into(),
                    display_name: None,
                    icon: None,
                    selected: true,
                    sort_order: 0,
                })
                .unwrap();
            let todos = TodoService::new(store);
            let blocker = todos
                .create(
                    1,
                    NewTodo {
                        title: "Prepare fixture".into(),
                        body: "Seed the shared state.".into(),
                        priority: TodoPriority::Low,
                        tags: vec!["setup".into()],
                    },
                    now,
                )
                .unwrap();
            let target = todos
                .create(
                    1,
                    NewTodo {
                        title: "Ship coordination UI".into(),
                        body: "## Acceptance\n\nWatch the plan update **live**.".into(),
                        priority: TodoPriority::High,
                        tags: vec!["ui".into(), "coordination".into()],
                    },
                    now,
                )
                .unwrap();
            todos.add_blocker(1, target.id, blocker.id, now).unwrap();
            todos
                .comment_create(1, target.id, "codex-w2", "Viewer wired.".into(), now)
                .unwrap();
            todos
                .lock(1, target.id, "codex-w2", 60_000_000, now)
                .unwrap();
            let scratchpads = ScratchpadService::new(store);
            scratchpads
                .write(
                    1,
                    None,
                    "Build plan".into(),
                    "# Build plan\n\nFirst revision.".into(),
                    Some(vec!["shared".into()]),
                    None,
                )
                .unwrap();
            let (archived_scratchpad, _) = scratchpads
                .write(
                    1,
                    None,
                    "Archived handoff".into(),
                    "Older notes kept for reference.".into(),
                    Some(vec!["archive".into()]),
                    None,
                )
                .unwrap();
            scratchpads
                .archive(
                    1,
                    archived_scratchpad.id,
                    Some(archived_scratchpad.revision),
                )
                .unwrap();
        }
        let (shutdown, receive_shutdown) = oneshot::channel();
        let task = tokio::spawn(server.serve_until(async move {
            let _ = receive_shutdown.await;
        }));
        Self {
            discovery,
            registry,
            shutdown: Some(shutdown),
            task,
            _temp: temp,
            _project_path: project_path,
        }
    }

    fn request(&self) -> tokio_tungstenite::tungstenite::http::Request<()> {
        let mut request = format!("ws://127.0.0.1:{}/ws", self.discovery.port)
            .into_client_request()
            .unwrap();
        request.headers_mut().insert(
            header::AUTHORIZATION,
            format!("Bearer {}", self.discovery.token).parse().unwrap(),
        );
        request
    }

    async fn stop(mut self) {
        self.shutdown.take().unwrap().send(()).unwrap();
        self.task.await.unwrap().unwrap();
    }
}

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

async fn rpc(socket: &mut Socket, id: &str, method: &str, params: Value) -> Value {
    socket
        .send(Message::Text(
            json!({ "id": id, "method": method, "params": params })
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    loop {
        let message = socket.next().await.unwrap().unwrap();
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

async fn rpc_error(socket: &mut Socket, id: &str, method: &str, params: Value) -> Value {
    socket
        .send(Message::Text(
            json!({ "id": id, "method": method, "params": params })
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    loop {
        let message = socket.next().await.unwrap().unwrap();
        let Message::Text(message) = message else {
            continue;
        };
        let response: Value = serde_json::from_str(&message).unwrap();
        if response["id"] == id {
            assert_eq!(
                response["ok"], false,
                "RPC unexpectedly succeeded: {response}"
            );
            return response["error"].clone();
        }
    }
}

#[tokio::test]
async fn coordination_rpcs_expose_board_detail_and_live_scratchpad_revisions() {
    let server = TestServer::start().await;
    let (mut socket, _) = connect_async(server.request()).await.unwrap();

    let snapshot = rpc(
        &mut socket,
        "snapshot-1",
        "coordination.snapshot",
        json!({ "project_id": 1 }),
    )
    .await;
    assert_eq!(snapshot["todo_total_count"], 2);
    assert_eq!(snapshot["scratchpad_total_count"], 1);
    assert_eq!(snapshot["archived_scratchpad_total_count"], 1);
    assert_eq!(
        snapshot["archived_scratchpads"][0]["name"],
        "Archived handoff"
    );
    assert_eq!(snapshot["archived_scratchpads"][0]["archived"], true);
    let target = snapshot["todos"]
        .as_array()
        .unwrap()
        .iter()
        .find(|todo| todo["title"] == "Ship coordination UI")
        .unwrap();
    assert_eq!(target["priority"], "high");
    assert_eq!(target["is_blocked"], true);
    assert_eq!(target["unresolved_blocker_count"], 1);
    assert_eq!(target["comment_count"], 1);
    assert_eq!(target["locked_by"], "codex-w2");
    let todo_id = target["id"].as_i64().unwrap();
    let fixture_id = target["blocker_ids"][0].as_i64().unwrap();
    let scratchpad_id = snapshot["scratchpads"][0]["id"].as_i64().unwrap();

    let created_scratchpad = rpc(
        &mut socket,
        "scratchpad-create",
        "coordination.scratchpad_create",
        json!({
            "project_id": 1,
            "name": "Release notes",
            "content": "First desktop-authored revision.",
            "tags": ["desktop"]
        }),
    )
    .await;
    assert_eq!(created_scratchpad["name"], "Release notes");
    assert_eq!(created_scratchpad["revision"], 1);
    assert_eq!(created_scratchpad["tags"], json!(["desktop"]));

    let updated_scratchpad = rpc(
        &mut socket,
        "scratchpad-update",
        "coordination.scratchpad_update",
        json!({
            "project_id": 1,
            "scratchpad_id": created_scratchpad["id"],
            "expected_revision": 1,
            "content": "# Launch notes\n\n### Desktop\n\nSaved as plain Markdown."
        }),
    )
    .await;
    assert_eq!(updated_scratchpad["scratchpad"]["name"], "Launch notes");
    assert_eq!(updated_scratchpad["scratchpad"]["revision"], 2);
    assert_eq!(
        updated_scratchpad["scratchpad"]["content"],
        "### Desktop\n\nSaved as plain Markdown."
    );
    let tagged_scratchpad = rpc(
        &mut socket,
        "scratchpad-set-tags",
        "coordination.scratchpad_set_tags",
        json!({
            "project_id": 1,
            "scratchpad_id": created_scratchpad["id"],
            "expected_revision": 2,
            "tags": ["desktop", "release"]
        }),
    )
    .await;
    assert_eq!(tagged_scratchpad["revision"], 3);
    assert_eq!(tagged_scratchpad["tags"], json!(["desktop", "release"]));
    let stale_update = rpc_error(
        &mut socket,
        "scratchpad-stale-update",
        "coordination.scratchpad_update",
        json!({
            "project_id": 1,
            "scratchpad_id": created_scratchpad["id"],
            "expected_revision": 1,
            "content": "# Stale draft\n\nThis must not replace revision 2."
        }),
    )
    .await;
    assert_eq!(stale_update["code"], "scratchpad_revision_conflict");

    let detail = rpc(
        &mut socket,
        "todo-detail",
        "coordination.todo",
        json!({ "project_id": 1, "todo_id": todo_id }),
    )
    .await;
    assert!(
        detail["todo"]["body"]
            .as_str()
            .unwrap()
            .contains("Acceptance")
    );
    assert_eq!(detail["comments"][0]["actor"], "codex-w2");

    let created = rpc(
        &mut socket,
        "todo-create",
        "coordination.todo_create",
        json!({
            "project_id": 1,
            "title": "Verify the desktop flow",
            "body": "Created from the project todo section.",
            "priority": "medium",
            "tags": ["desktop"],
            "blocker_ids": [todo_id]
        }),
    )
    .await;
    let created_id = created["id"].as_i64().unwrap();
    assert_eq!(created["status"], "open");
    assert_eq!(created["blocker_ids"], json!([todo_id]));
    assert_eq!(created["is_blocked"], true);

    let todo_reorder = rpc(
        &mut socket,
        "todo-reorder",
        "coordination.todo_reorder",
        json!({
            "project_id": 1,
            "ordered_ids": [created_id, todo_id, fixture_id]
        }),
    )
    .await;
    let todo_sort = todo_reorder["todos"]
        .as_array()
        .unwrap()
        .iter()
        .map(|todo| {
            (
                todo["id"].as_i64().unwrap(),
                todo["sort_order"].as_i64().unwrap(),
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(todo_sort[&created_id], 0);
    assert_eq!(todo_sort[&todo_id], 1);
    assert_eq!(todo_sort[&fixture_id], 2);

    let scratchpad_reorder = rpc(
        &mut socket,
        "scratchpad-reorder",
        "coordination.scratchpad_reorder",
        json!({
            "project_id": 1,
            "ordered_ids": [created_scratchpad["id"], scratchpad_id]
        }),
    )
    .await;
    assert_eq!(
        scratchpad_reorder["scratchpads"][0]["id"],
        created_scratchpad["id"]
    );
    assert_eq!(scratchpad_reorder["scratchpads"][0]["sort_order"], 0);

    let added_blocker = rpc(
        &mut socket,
        "todo-add-blocker",
        "coordination.todo_add_blocker",
        json!({
            "project_id": 1,
            "todo_id": created_id,
            "blocker_id": fixture_id
        }),
    )
    .await;
    assert_eq!(added_blocker["blocker_ids"], json!([fixture_id, todo_id]));
    let removed_blocker = rpc(
        &mut socket,
        "todo-remove-blocker",
        "coordination.todo_remove_blocker",
        json!({
            "project_id": 1,
            "todo_id": created_id,
            "blocker_id": fixture_id
        }),
    )
    .await;
    assert_eq!(removed_blocker["blocker_ids"], json!([todo_id]));
    let set_blockers = rpc(
        &mut socket,
        "todo-set-blockers",
        "coordination.todo_set_blockers",
        json!({
            "project_id": 1,
            "todo_id": created_id,
            "blocker_ids": [fixture_id]
        }),
    )
    .await;
    assert_eq!(set_blockers["blocker_ids"], json!([fixture_id]));
    let cycle = rpc_error(
        &mut socket,
        "todo-blocker-cycle",
        "coordination.todo_set_blockers",
        json!({
            "project_id": 1,
            "todo_id": fixture_id,
            "blocker_ids": [todo_id]
        }),
    )
    .await;
    assert_eq!(cycle["code"], "todo_blocker_cycle");

    let updated = rpc(
        &mut socket,
        "todo-update",
        "coordination.todo_update",
        json!({
            "project_id": 1,
            "todo_id": created_id,
            "title": "Verify the document flow",
            "priority": "high",
            "tags": ["desktop", "document"]
        }),
    )
    .await;
    assert_eq!(updated["title"], "Verify the document flow");
    assert_eq!(updated["priority"], "high");
    assert_eq!(updated["tags"], json!(["desktop", "document"]));

    let locked = rpc(
        &mut socket,
        "todo-lock",
        "coordination.todo_lock",
        json!({ "project_id": 1, "todo_id": created_id }),
    )
    .await;
    assert_eq!(locked["locked_by"], "desktop-ui");
    let unlocked = rpc(
        &mut socket,
        "todo-unlock",
        "coordination.todo_unlock",
        json!({ "project_id": 1, "todo_id": created_id }),
    )
    .await;
    assert_eq!(unlocked["locked_by"], Value::Null);

    let comment = rpc(
        &mut socket,
        "todo-comment",
        "coordination.todo_comment",
        json!({
            "project_id": 1,
            "todo_id": created_id,
            "body": "The visible comment composer works."
        }),
    )
    .await;
    assert_eq!(comment["actor"], "desktop-ui");

    let completed = rpc(
        &mut socket,
        "todo-complete",
        "coordination.todo_complete",
        json!({ "project_id": 1, "todo_id": created_id, "completed": true }),
    )
    .await;
    assert_eq!(completed["todo"]["completed"], true);
    assert_eq!(completed["todo"]["status"], "completed");

    let created_detail = rpc(
        &mut socket,
        "created-detail",
        "coordination.todo",
        json!({ "project_id": 1, "todo_id": created_id }),
    )
    .await;
    assert_eq!(created_detail["comment_total_count"], 1);
    assert_eq!(
        created_detail["comments"][0]["body"],
        "The visible comment composer works."
    );
    assert_eq!(
        created_detail["activity"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["kind"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["created", "locked", "unlocked", "completed"]
    );

    let first_read = rpc(
        &mut socket,
        "scratchpad-1",
        "coordination.scratchpad",
        json!({ "project_id": 1, "scratchpad_id": scratchpad_id }),
    )
    .await;
    assert_eq!(first_read["scratchpad"]["revision"], 1);
    assert_eq!(first_read["scratchpad"]["name"], "Build plan");

    {
        let registry = server.registry.lock().await;
        ScratchpadService::new(registry.store())
            .write(
                1,
                Some(scratchpad_id),
                "Build plan".into(),
                "# Build plan\n\nSecond revision from another agent.".into(),
                None,
                Some(1),
            )
            .unwrap();
    }
    let second_snapshot = rpc(
        &mut socket,
        "snapshot-2",
        "coordination.snapshot",
        json!({ "project_id": 1 }),
    )
    .await;
    let refreshed = second_snapshot["scratchpads"]
        .as_array()
        .unwrap()
        .iter()
        .find(|scratchpad| scratchpad["id"] == scratchpad_id)
        .unwrap();
    assert_eq!(refreshed["revision"], 2);
    let second_read = rpc(
        &mut socket,
        "scratchpad-2",
        "coordination.scratchpad",
        json!({ "project_id": 1, "scratchpad_id": scratchpad_id }),
    )
    .await;
    assert_eq!(second_read["scratchpad"]["revision"], 2);
    assert!(
        second_read["scratchpad"]["content"]
            .as_str()
            .unwrap()
            .contains("another agent")
    );

    socket.close(None).await.unwrap();
    server.stop().await;
}
