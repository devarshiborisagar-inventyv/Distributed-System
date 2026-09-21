use crate::cache::{Data, KeyReq, KeyRes, SetReq, SetReqFollower, Store , SetReqFollowerPayload};
use axum::{Json, extract::State};
use chrono::Utc;
use futures::future::join_all;
use crate::handlers::encrypt::{sign_data,string_to_signing_key};

const SIGNING_KEY: &str = "9YKJhQLFlv-zxpamKNX5ioFRsZ0vNWEMbo8fCqQmCWU";

#[derive(Clone)]
pub struct AppState {
    pub data: Store,
    pub followers_list: Vec<String>,
}

pub async fn hello() -> &'static str {
    "Hello"
}

pub async fn set_data(state: State<AppState>, data: Json<SetReq>) -> &'static str {
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
     let data_to_send_to_followers = SetReqFollower {
        encrypted_payload:sign_data(
            &serde_json::to_string(&data_to_send_to_followers_payload).unwrap(),
            &string_to_signing_key(&SIGNING_KEY).unwrap(),
        ),
    };
    

    state
        .data
        .lock()
        .unwrap()
        .insert(data.key.clone(), data_to_insert.clone());

    //Loop over followersList and send req to all to update
    let sends = state.followers_list.iter().map(|follower| {
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
    "Data is set"
}

pub async fn get_data(state: State<AppState>, data: Json<KeyReq>) -> Json<KeyRes> {
    let mut map = state.data.lock().unwrap();
    let key = data.key.clone();
    println!("Data is get key:{}", &key);

    if map.get(&key).unwrap().is_expired() {
        map.remove(&key);
        return Json(KeyRes {
            message: "Key is expired".to_string(),
            key,
            value: None,
        });
    }

    Json(match map.get(&key).filter(|d| !d.is_expired()) {
        Some(value) => KeyRes {
            message: "Data is Found".to_string(),
            key,
            value: Some(value.clone()),
        },
        None => KeyRes {
            message: "Key not found".to_string(),
            key,
            value: None,
        },
    })
}

pub async fn delete_data(state: State<AppState>, data: Json<KeyReq>) -> Json<KeyRes> {
    let removed = state.data.lock().unwrap().remove(&data.key);
    let key = data.key.clone();
    println!("Data is deleted key:{}", &key);

    // fan the delete out too, otherwise followers keep the stale value forever
    let sends = state.followers_list.iter().map(|follower| {
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
