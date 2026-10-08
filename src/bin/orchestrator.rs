use axum::{
    Router,
    routing::{get, post},
};
use mini_db::handlers::orchestrator_handlers::{AppState, hello, init_cluster};

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(hello))
        .route("/init", post(init_cluster))
        .with_state(AppState::default());

    // 4000, not 3000: the leader binds 3000 and the orchestrator must not be
    // holding it when it spawns one.
    let listener = tokio::net::TcpListener::bind("0.0.0.0:4000")
        .await
        .unwrap();

    println!("orchestrator listening on :4000");
    axum::serve(listener, app).await.unwrap();
}
