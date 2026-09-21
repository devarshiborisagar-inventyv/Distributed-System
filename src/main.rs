use mini_db::cache::start_passive_cleaner;
use mini_db::handlers::leader_handlers::{
    AppState,
    get_data,
    hello,
    set_data,
    delete_data,
};
use axum::{Router, routing::{get, post}};
use std::{collections::HashMap, sync::{Arc, Mutex}};

#[tokio::main]
async fn main() {
    let state = AppState {
        data: Arc::new(Mutex::new(HashMap::new())),
        followers_list:vec!["localhost:3001".to_string(),"localhost:3002".to_string(),"localhost:3003".to_string()]
    };

    tokio::spawn(start_passive_cleaner("leader".to_string(), state.data.clone()));

    let app = Router::new()
        .route("/", get(hello))
        .route("/set", post(set_data))
        .route("/get", post(get_data))
        .route("/delete", post(delete_data))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .unwrap();

    axum::serve(listener, app).await.unwrap();
}