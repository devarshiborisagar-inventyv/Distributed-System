use crate::cache::{Data, InitReqLeader, KeyReq, KeyRes, SetReq, SetReqFollower, SetReqFollowerPayload, Store};
use crate::error::{AppError, lock, read, write};
use crate::handlers::encrypt::{sign_data, string_to_signing_key};
use axum::{Json, extract::State};
use chrono::Utc;
use futures::stream::{FuturesUnordered, StreamExt};
use serde::Serialize;
use std::sync::{Arc, RwLock};
use tokio::time::{Duration, sleep};

/// How long the leader will wait for followers before answering the client.
const FANOUT_DEADLINE: Duration = Duration::from_millis(500);

/// Replicate one operation to every follower and return how many accepted it.
///
/// Returns as soon as a majority of the *cluster* has it (the leader is
/// already one member, so it needs `followers/2` more), or when the deadline
/// passes — whichever comes first. One frozen follower therefore costs the
/// deadline, not the request.
///
/// Every caller that mutates state must go through here. Giving `/set` its own
/// copy is how `/delete` ended up with no timeout, no shared client and no
/// error reporting.
async fn replicate<B: Serialize>(
    http_client: &reqwest::Client,
    followers: &[String],
    path: &str,
    body: &B,
) -> usize {
    if followers.is_empty() {
        return 0;
    }

    let mut sends: FuturesUnordered<_> = followers
        .iter()
        .map(|follower| {
            http_client
                .post(format!("http://{follower}{path}"))
                .json(body)
                .send()
        })
        .collect();

    // Majority of (leader + followers), with the leader counted as an ack.
    let needed = (followers.len() + 1) / 2;
    let mut accepted = 0usize;

    let deadline = sleep(FANOUT_DEADLINE);
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            Some(result) = sends.next() => {
                match result {
                    // reqwest calls a 4xx/5xx reply `Ok`, so the status has to
                    // be checked explicitly or a rejection counts as an ack.
                    Ok(r) if r.status().is_success() => accepted += 1,
                    Ok(r) => eprintln!("[leader] follower rejected {path}: HTTP {}", r.status()),
                    Err(e) => eprintln!("[leader] follower failed {path}: {e}"),
                }
                if accepted >= needed || sends.is_empty() {
                    break;
                }
            }
            _ = &mut deadline => {
                eprintln!(
                    "[leader] {path} deadline hit: {accepted}/{} followers acked, needed {needed}",
                    followers.len()
                );
                break;
            }
        }
    }

    accepted
}
#[derive(Clone)]
pub struct AppState {
    pub signing_key: Arc<RwLock<String>>,
    pub data: Store,
    pub followers_list: Arc<RwLock<Vec<String>>>,
    pub active_follower_list:Vec<String>,
    pub http_client:reqwest::Client
}

pub async fn hello() -> &'static str {
    "Hello"
}


pub async fn init_node(state: State<AppState>, data: Json<InitReqLeader>)-> &'static str{

    *write(&state.signing_key) = data.secrete.clone();
    // replace, not append: a retried init must not double the list
    *write(&state.followers_list) = data.follwers_list.clone();
    "Node is Initiated succsfully"
}

pub async fn set_data(
    state: State<AppState>,
    data: Json<SetReq>,
) -> Result<&'static str, AppError> {

    let http_client = state.http_client.clone();

    let data_to_insert = Data {
        value: data.value.clone(),
        created_at: Utc::now(),
        experied_in: data.expire.unwrap_or(0),
    };
    let data_to_send_to_followers_payload = SetReqFollowerPayload {
        key: data.key.clone(),
        value: data.value.clone(),
        created_at: data_to_insert.created_at,
        expire: data.expire.unwrap_or(0),
    };
    // Scoped so the guard is dropped before the fan-out await below.
    let signing_key = {
        let stored = read(&state.signing_key);
        string_to_signing_key(&stored)?
    };
    let data_to_send_to_followers = SetReqFollower {
        encrypted_payload: sign_data(
            &serde_json::to_string(&data_to_send_to_followers_payload)?,
            &signing_key,
        ),
    };

    lock(&state.data).insert(data.key.clone(), data_to_insert.clone());

    let followers = read(&state.followers_list).clone();
    replicate(&http_client, &followers, "/set", &data_to_send_to_followers).await;

    println!(
        "Data is set key:{} value:{:?} in leader node",
        &data.key, &data.value
    );
    Ok("Data is set")
}

pub async fn get_data(state: State<AppState>, data: Json<KeyReq>) -> Json<KeyRes> {
    let mut map = lock(&state.data);
    let key = data.key.clone();
    println!("Data is get key: {}", &key);
    match map.get(&key) {
        Some(value) if value.is_expired() => {
            map.remove(&key);

            Json(KeyRes {
                message: "Key is expired".to_string(),
                key,
                value: None,
            })
        }
        Some(value) => {
            Json(KeyRes {
                message: "Data is Found".to_string(),
                key,
                value: Some(value.clone()),
            })
        }
        None => {
            Json(KeyRes {
                message: "Key not found".to_string(),
                key,
                value: None,
            })
        }
    }
}

pub async fn delete_data(state: State<AppState>, data: Json<KeyReq>) -> Json<KeyRes> {
    let removed = lock(&state.data).remove(&data.key);
    let key = data.key.clone();
    println!("Data is deleted key:{}", &key);

    // fan the delete out too, otherwise followers keep the stale value forever
    let followers = read(&state.followers_list).clone();
    replicate(&state.http_client, &followers, "/delete", &data.0).await;

    Json(match removed {
        Some(value) => KeyRes {
            message: "Data is Deleted".to_string(),
            key,
            value: Some(value),
        },
        None => KeyRes {
            message: "Key not found".to_string(),
            key,
            value: None,
        },
    })
}

