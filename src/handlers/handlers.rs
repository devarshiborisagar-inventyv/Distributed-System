use std::sync::RwLock;

use crate::cache::{KeyReq, KeyRes, SetReqFollower, Store,Data,SetReqFollowerPayload,SetKeyReq};
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
    println!("Data is get key:{}", &key);
    if map.get(&key).unwrap().is_expired(){
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


pub async fn save_key(state:State<AppState>,data:Json<SetKeyReq>)-> &'static str{
    
    let data_clone = data.clone();

    *state.verifying_key.write().await = data_clone.security_key.to_string();

    return "Key is saved";
}
