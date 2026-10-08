use crate::cache::{Data, InitReqLeader, KeyReq, KeyRes, SetReq, SetReqFollower, SetReqFollowerPayload, Store};
use crate::error::{AppError, lock, read, write};
use crate::handlers::encrypt::{sign_data, string_to_signing_key};
use axum::{Json, extract::State};
use chrono::Utc;
use futures::future::join_all;
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct AppState {
    pub signing_key: Arc<RwLock<String>>,
    pub data: Store,
    pub followers_list: Arc<RwLock<Vec<String>>>,
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

    //Loop over followersList and send req to all to update
    let followers = read(&state.followers_list).clone();
    let sends = followers.iter().map(|follower| {
        reqwest::Client::new()
            .post(format!("http://{}/set", follower))
            .json(&data_to_send_to_followers)
            .send() // a Future that BORROWS follower & data
    });
    join_all(sends).await;
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
    let sends = followers.iter().map(|follower| {
        reqwest::Client::new()
            .post(format!("http://{}/delete", follower))
            .json(&data.0)
            .send()
    });
    join_all(sends).await;

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

