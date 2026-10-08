//! One error type for every fallible path, so a bad request returns a status
//! code instead of unwinding a worker thread.
use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

#[derive(Debug)]
pub enum AppError {
    /// Node is up but the orchestrator hasn't pushed its keys yet.
    NotInitialized,
    /// The server's own stored key couldn't be parsed.
    InvalidKey,
    /// Malformed or forged token.
    InvalidToken,
    /// Signature OK but the JSON doesn't match the struct.
    InvalidPayload,
    ClusterAlreadyStarted,
    TooManyReplicas(u32),
    NodeNotReady(String),
    Spawn(std::io::Error),
    Upstream(reqwest::Error),
}

impl AppError {
    pub fn status(&self) -> StatusCode {
        match self {
            // 503, not 500: the caller should retry once init lands.
            Self::NotInitialized => StatusCode::SERVICE_UNAVAILABLE,
            Self::InvalidKey | Self::Spawn(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::InvalidToken => StatusCode::UNAUTHORIZED,
            Self::InvalidPayload | Self::TooManyReplicas(_) => StatusCode::BAD_REQUEST,
            Self::ClusterAlreadyStarted => StatusCode::CONFLICT,
            Self::NodeNotReady(_) => StatusCode::GATEWAY_TIMEOUT,
            Self::Upstream(_) => StatusCode::BAD_GATEWAY,
        }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotInitialized => write!(f, "node has not been initialised yet"),
            Self::InvalidKey => write!(f, "invalid key"),
            Self::InvalidToken => write!(f, "invalid token"),
            Self::InvalidPayload => write!(f, "invalid payload"),
            Self::ClusterAlreadyStarted => write!(f, "cluster already started"),
            Self::TooManyReplicas(n) => write!(f, "{n} replicas does not fit the port range"),
            Self::NodeNotReady(n) => write!(f, "{n} never became ready"),
            Self::Spawn(e) => write!(f, "could not spawn node: {e}"),
            Self::Upstream(e) => write!(f, "node call failed: {e}"),
        }
    }
}

impl std::error::Error for AppError {}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self::Spawn(e)
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        Self::Upstream(e)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(_: serde_json::Error) -> Self {
        Self::InvalidPayload
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        // Log our own faults; a client's bad request is the client's problem.
        if status.is_server_error() {
            eprintln!("[error] {self}");
        }
        (status, Json(json!({ "error": self.to_string() }))).into_response()
    }
}

// --- poison-safe locking ---------------------------------------------------
// A panic while a lock is held poisons it, and `.lock().unwrap()` then panics
// for every later caller — one bad request kills the node for good. Recovering
// the guard beats reporting the poisoning: the data may be mid-update, but the
// node stays serving. That is why there is no `LockPoisoned` variant.

pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn read<T>(l: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    l.read().unwrap_or_else(|e| e.into_inner())
}

pub fn write<T>(l: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    l.write().unwrap_or_else(|e| e.into_inner())
}
