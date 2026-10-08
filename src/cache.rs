// Shared cache types + the expiry sweeper. Used by both the leader (main.rs)
// and the followers (bin/follower.rs).
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use crate::error::lock;
use tokio::time::{Duration, sleep};




/// A cached value. `experied_in` is the TTL in **seconds**; 0 = never expires.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Data {
    pub value: String,
    pub created_at: DateTime<Utc>,
    pub experied_in: u128,
}

impl Data {
    pub fn is_expired(&self) -> bool {
        if self.experied_in == 0 {
            return false;
        }
        (Utc::now() - self.created_at).num_seconds().max(0) as u128 >= self.experied_in
    }
}

pub type Store = Arc<Mutex<HashMap<String, Data>>>;

#[derive(Serialize, Deserialize, Clone)]
pub struct InitReqLeader {
    pub secrete: String,
    pub follwers_list: Vec<String>,
}
#[derive(Serialize, Deserialize, Clone)]
pub struct InitReqFollower {
    pub secrete: String,
}


#[derive(Serialize, Deserialize, Clone)]
pub struct SetReq {
    pub key: String,
    pub value: String,
    pub expire:Option<u128>
}
#[derive(Serialize, Deserialize, Clone)]
pub struct SetReqFollowerPayload {
    pub key: String,
    pub value: String,
    pub created_at: DateTime<Utc>,
    pub expire:u128,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SetReqFollower {
    pub encrypted_payload: String,
}

/// GET and DELETE take the same body.
#[derive(Serialize, Deserialize, Clone)]
pub struct KeyReq {
    pub key: String,
}

/// GET and DELETE return the same shape; `value` is null on a miss.
#[derive(Serialize)]
pub struct KeyRes {
    pub message: String,
    pub key: String,
    pub value: Option<Data>,
}
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "UPPERCASE")]
pub enum Role {
    Leader,
    Follower,
    Orchestrator,
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Node{
    pub ip: String,
    pub port: u16,
    pub role: Role,
    pub node_id: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct InitReqBody{
    pub replication_count: u16,
}
/// Active expiry: drop expired keys every 10s. Lazy expiry on GET still
/// matters — this only bounds how long dead entries hold memory.
pub async fn start_passive_cleaner(role: Role, store: Store) {
    loop {
        sleep(Duration::from_secs(10)).await;
        let mut map = lock(&store);
        let before = map.len();
        map.retain(|_, d| !d.is_expired());
        if before != map.len() {
            println!("[{role}] cleaner removed {} expired keys", before - map.len());
        }
    }
}
