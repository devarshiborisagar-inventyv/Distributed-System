use axum::{
    Router,
    routing::{get, post},
};
use mini_db::cache::{Role, start_passive_cleaner};
use mini_db::handlers::handlers::{AppState, delete_data, get_data, hello, set_data, init_node};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use std::sync::RwLock;

#[tokio::main]
async fn main() {
    let state = AppState {
        verifying_key: Arc::new(RwLock::new(String::from(""))),
        data: Arc::new(Mutex::new(HashMap::new())),
    };

    tokio::spawn(start_passive_cleaner(Role::Follower, state.data.clone()));

    let app = Router::new()
        .route("/", get(hello))
        .route("/set", post(set_data))
        .route("/get", post(get_data))
        .route("/delete", post(delete_data))
        .route("/init_data", post(init_node))
        .with_state(state);

    let port = std::env::args().nth(1).unwrap_or_else(|| "3001".to_string());
    let listener = match tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await {
        Ok(l) => l,
        Err(e) => {
            // A bind clash is the usual cause and the panic hid it. Say so.
            eprintln!("[follower] cannot bind :{port}: {e}");
            std::process::exit(1);
        }
    };

    println!("follower listening on :{port}");
    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("[follower] server stopped: {e}");
        std::process::exit(1);
    }
}
