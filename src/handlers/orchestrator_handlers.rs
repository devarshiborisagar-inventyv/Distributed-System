use axum::{Json, extract::State};
use crate::cache::{InitReqBody, Node, Role};
use crate::error::{AppError, lock};
use crate::handlers::generate_keys::generate_keys;
use futures::future::try_join_all;
use serde_json::json;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};
use uuid::Uuid;

/// `children` holds the spawned `Child` handles: `kill_on_drop(true)` means
/// dropping one SIGKILLs that node, so they must outlive the request.
#[derive(Clone)]
pub struct AppState {
    pub nodes: Arc<Mutex<Vec<Node>>>,
    pub children: Arc<Mutex<Vec<Child>>>,
    pub client: reqwest::Client,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            nodes: Arc::new(Mutex::new(Vec::new())),
            children: Arc::new(Mutex::new(Vec::new())),
            client: reqwest::Client::new(),
        }
    }
}

pub async fn hello() -> &'static str {
    "Hello"
}

/// Spawn a cluster, wait for every node to answer, then push its config in.
pub async fn init_cluster(
    state: State<AppState>,
    req: Json<InitReqBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !lock(&state.nodes).is_empty() {
        return Err(AppError::ClusterAlreadyStarted);
    }

    let keys = generate_keys();

    let leader_node = Node {
        ip: "localhost".to_string(),
        node_id: Uuid::new_v4().to_string(),
        port: 3000,
        role: Role::Leader,
    };

    // `3000 + i` overflows u16 well before it runs out of ports; checked_add
    // turns a silly request into a 400 instead of a dead orchestrator.
    let follower_nodes: Vec<Node> = (1..=req.replication_count)
        .map(|i| {
            Ok(Node {
                ip: "localhost".to_string(),
                node_id: Uuid::new_v4().to_string(),
                port: 3000u16
                    .checked_add(i)
                    .ok_or(AppError::TooManyReplicas(u32::from(req.replication_count)))?,
                role: Role::Follower,
            })
        })
        .collect::<Result<_, AppError>>()?;

    let children = start_setup_leader_follower(&leader_node, &follower_nodes).await?;
    lock(&state.children).extend(children);

    // A spawned process is not a listening process. Push config before the
    // node binds its port and every request is a connection refused.
    for node in std::iter::once(&leader_node).chain(follower_nodes.iter()) {
        if !wait_ready(&state.client, node, Duration::from_secs(5)).await {
            return Err(AppError::NodeNotReady(format!("{}:{}", node.role, node.port)));
        }
    }

    // Followers first: configure the leader first and it can accept a write
    // and fan out to followers that have no verifying key yet.
    let mut pushes = Vec::new();
    for node in &follower_nodes {
        pushes.push(
            state
                .client
                .post(format!("http://{}:{}/init_data", node.ip, node.port))
                .json(&json!({ "secrete": keys.verifying }))
                .send(),
        );
    }
    let follower_addrs: Vec<String> = follower_nodes
        .iter()
        .map(|n| format!("{}:{}", n.ip, n.port))
        .collect();
    pushes.push(
        state
            .client
            .post(format!(
                "http://{}:{}/init_data",
                leader_node.ip, leader_node.port
            ))
            .json(&json!({
                "secrete": keys.signing,
                "follwers_list": follower_addrs,
            }))
            .send(),
    );

    // reqwest calls a 404 `Ok`, so error_for_status is what actually catches a
    // missing /init_data route.
    for res in try_join_all(pushes).await? {
        res.error_for_status()?;
    }

    let mut nodes = lock(&state.nodes);
    nodes.push(leader_node);
    nodes.extend(follower_nodes);

    Ok(Json(json!({
        "message": "cluster started",
        "nodes": *nodes,
    })))
}

/// Poll a node's `/` until it answers or the deadline passes.
async fn wait_ready(client: &reqwest::Client, node: &Node, timeout: Duration) -> bool {
    let url = format!("http://{}:{}/", node.ip, node.port);
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        let ok = client
            .get(&url)
            .timeout(Duration::from_millis(200))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success());
        if ok {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

/// Spawn the leader and every follower as child processes. Returns their
/// handles — the caller MUST keep them alive (`kill_on_drop`).
///
/// Binaries are resolved next to the running orchestrator, so this works from
/// any cwd and picks up debug/release automatically.
pub async fn start_setup_leader_follower(
    leader_node: &Node,
    follower_nodes: &[Node],
) -> Result<Vec<Child>, AppError> {
    let exe = std::env::current_exe()?;
    let dir = exe.parent().ok_or_else(|| {
        std::io::Error::other("running binary has no parent directory")
    })?;
    let bin = |name: &str| dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));

    let mut children = Vec::with_capacity(follower_nodes.len() + 1);

    // main.rs hardcodes 0.0.0.0:3000, so the leader takes no port argument.
    // Pass leader_node.port here once main.rs accepts --listen.
    children.push(spawn_node(bin("mini-db"), &[], tag(leader_node))?);

    for node in follower_nodes {
        children.push(spawn_node(
            bin("follower"),
            &[node.port.to_string()],
            tag(node),
        )?);
    }

    Ok(children)
}

fn tag(node: &Node) -> String {
    format!("{}:{}", node.role, node.port)
}

fn spawn_node(bin: PathBuf, args: &[String], tag: String) -> std::io::Result<Child> {
    let mut child = Command::new(&bin)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    println!("[orchestrator] spawned {} pid={:?}", tag, child.id());

    // Without a prefix, four nodes interleaving on one terminal is unreadable.
    // stderr matters as much as stdout: panics go there.
    if let Some(out) = child.stdout.take() {
        forward(out, tag.clone());
    }
    if let Some(err) = child.stderr.take() {
        forward(err, tag);
    }
    Ok(child)
}

fn forward<R: AsyncRead + Unpin + Send + 'static>(reader: R, tag: String) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            println!("[{tag}] {line}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // Checks the spawn + output-forwarding path without needing the cluster
    // binaries built: /bin/echo stands in for a node.
    #[tokio::test]
    async fn spawns_and_forwards_output() {
        let mut child = spawn_node(
            PathBuf::from("/bin/echo"),
            &["hello from node".to_string()],
            "TEST:0".to_string(),
        )
        .expect("spawn failed");
        let status = child.wait().await.expect("wait failed");
        assert!(status.success(), "child exited with {status:?}");
    }
}