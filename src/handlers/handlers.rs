use crate::cache::{Data, InitReqFollower, KeyReq, KeyRes, SetReqFollower, SetReqFollowerPayload, Store};
use crate::error::{AppError, lock, read, write};
use crate::handlers::encrypt::{string_to_verifying_key, verify_data};
use axum::{Json, extract::State};
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct AppState {
    pub verifying_key: Arc<RwLock<String>>,
    pub data: Store,
}

pub async fn hello() -> &'static str {
    "Hello"
}

pub async fn init_node(state: State<AppState>, data: Json<InitReqFollower>) -> &'static str {
    *write(&state.verifying_key) = data.secrete.clone();
    "Node is Initiated succsfully"
}

pub async fn set_data(state: State<AppState>, data: Json<SetReqFollower>) -> Result<&'static str, AppError> {
    // Scoped so the guard is dropped before anything else runs.
    let verifying_key = {
        let stored = read(&state.verifying_key);
        string_to_verifying_key(&stored)?
    };

    let payload = verify_data(&data.encrypted_payload, &verifying_key)?;
    let decoded_payload = serde_json::from_str::<SetReqFollowerPayload>(&payload)?;

    let data_to_insert = Data {
        value: decoded_payload.value.clone(),
        created_at: decoded_payload.created_at,
        experied_in: decoded_payload.expire,
    };

    lock(&state.data).insert(decoded_payload.key.clone(), data_to_insert);
    println!("Data is set key:{} value:{:?}", &decoded_payload.key, &decoded_payload.value);
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
    let mut map = lock(&state.data);
    let key = data.key.clone();
    println!("Data is deleted key:{}", &key);
    Json(match map.remove(&key) {
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

