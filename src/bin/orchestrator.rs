use axum::{
    Router,
    routing::{get, post},
};
use mini_db::handlers::orchestrator_handlers::{AppState, hello, init_cluster};

#[tokio::main]
async fn main() {
    // 4000, not 3000: the leader binds 3000 and the orchestrator must not be
    // holding it when it spawns one.
    const ADDR: &str = "0.0.0.0:4000";

    let app = Router::new()
        .route("/", get(hello))
        .route("/init", post(init_cluster))
        .with_state(AppState::default());

    let listener = match tokio::net::TcpListener::bind(ADDR).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[orchestrator] cannot bind {ADDR}: {e}");
            std::process::exit(1);
        }
    };

    println!("orchestrator listening on {ADDR}");
    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("[orchestrator] server stopped: {e}");
        std::process::exit(1);
    }
}
