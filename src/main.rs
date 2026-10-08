use mini_db::cache::{Role, start_passive_cleaner};
use mini_db::handlers::leader_handlers::{
    AppState,
    get_data,
    hello,
    set_data,
    delete_data,
    init_node,
};
use axum::{Router, routing::{get, post}};
use std::{collections::HashMap, sync::{Arc, Mutex, RwLock}};

#[tokio::main]
async fn main() {
    const ADDR: &str = "0.0.0.0:3000";

    let state = AppState {
        signing_key: Arc::new(RwLock::new(String::new())),
        data: Arc::new(Mutex::new(HashMap::new())),
        followers_list: Arc::new(RwLock::new(vec![]))
    };

    tokio::spawn(start_passive_cleaner(Role::Leader, state.data.clone()));

    let app = Router::new()
        .route("/", get(hello))
        .route("/set", post(set_data))
        .route("/get", post(get_data))
        .route("/delete", post(delete_data))
        .route("/init_data", post(init_node))
        .with_state(state);

    let listener = match tokio::net::TcpListener::bind(ADDR).await {
        Ok(l) => l,
        Err(e) => {
            // A bind clash is the usual cause and the panic hid it. Say so.
            eprintln!("[leader] cannot bind {}: {e}", ADDR);
            std::process::exit(1);
        }
    };

    println!("leader listening on {ADDR}");
    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("[leader] server stopped: {e}");
        std::process::exit(1);
    }
}