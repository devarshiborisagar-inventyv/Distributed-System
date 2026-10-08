use std::sync::RwLock;

use crate::cache::{Data, InitReqFollower, KeyReq, KeyRes, SetReqFollower, SetReqFollowerPayload, Store};
use axum::{Json, extract::State};
use crate::handlers::encrypt::{verify_data,string_to_verifying_key};
use std::{
    sync::{Arc},
};
// static  mut VERIFYING_KEY:String = String::from("1Ps9YkDqB5j876JgwV0M5K1CXk9qKUtFw14c4NXnoUc");

#[derive(Clone)]
pub struct AppState {
    pub verifying_key: Arc<RwLock<String>>,
    pub data: Store,
}

pub async fn hello() -> &'static str {
    "Hello"
}

pub async fn init_node(state: State<AppState>, data: Json<InitReqFollower>)-> &'static str{

    let mut verifying_key=state.verifying_key.write().unwrap();
    *verifying_key=data.secrete.clone();
    "Node is Initiated succsfully"
}

pub async fn set_data(state: State<AppState>, data: Json<SetReqFollower>) -> &'static str {

    let payload = verify_data(&data.encrypted_payload, &string_to_verifying_key(&state.verifying_key.read().unwrap()).unwrap());

    let decoded_payload=serde_json::from_str::<SetReqFollowerPayload>(&payload.unwrap()).unwrap();

    let data_to_insert = Data {
        value: decoded_payload.value.clone(),
        created_at: decoded_payload.created_at,
        experied_in: decoded_payload.expire,
    };

    state
        .data
        .lock()
        .unwrap()
        .insert(decoded_payload.key.clone(), data_to_insert);
    println!("Data is set key:{} value:{:?}", &decoded_payload.key, &decoded_payload.value);
    "Data is set"
}

pub async fn get_data(state: State<AppState>, data: Json<KeyReq>) -> Json<KeyRes> {
    let mut map = state.data.lock().unwrap();
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
    let mut map = state.data.lock().unwrap();
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

