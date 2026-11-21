//! Controle do host: LiveKit sidecar, JWT tokens, RoomService Twirp API,
//! rate-limiting de /api/join, teste de alcance e fila de sucessão.

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ct_core::protocol::{MAX_PARTICIPANTS, QualityId, ROOM_NAME, RoomMeta, RoomScope};
use ct_net::ip::is_private_ip;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use subtle::ConstantTimeEq;
use thiserror::Error;
use tokio::sync::Mutex;

use crate::livekit_process::{LiveKitError, LiveKitPaths, LiveKitProcess};
use crate::probe::probe_candidate;

const MAX_FAILED_JOINS: u32 = 8;
const FAILED_JOIN_WINDOW: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone)]
pub struct HostStartOptions {
    pub secret: u32,
    pub epoch: u64,
    pub host_id: String,
    pub host_name: String,
    pub public_ip: Option<String>,
    pub node_ip: Option<String>,
    pub code: String,
}

#[derive(Debug, Deserialize)]
pub struct JoinRequest {
    pub secret: Option<u32>,
    pub identity: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HttpReply {
    pub status: u16,
    pub body: Value,
}

#[derive(Debug, Error)]
pub enum HostError {
    #[error("LiveKit: {0}")]
    LiveKit(#[from] LiveKitError),
    #[error("JWT: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),
    #[error("HTTP: {0}")]
    Http(#[from] reqwest::Error),
    #[error("RoomService retornou status {0}")]
    RoomServiceStatus(u16),
}

#[derive(Serialize)]
struct VideoGrant {
    room: String,
    #[serde(rename = "roomJoin", skip_serializing_if = "Option::is_none")]
    room_join: Option<bool>,
    #[serde(rename = "roomCreate", skip_serializing_if = "Option::is_none")]
    room_create: Option<bool>,
    #[serde(rename = "roomAdmin", skip_serializing_if = "Option::is_none")]
    room_admin: Option<bool>,
    #[serde(rename = "canSubscribe", skip_serializing_if = "Option::is_none")]
    can_subscribe: Option<bool>,
    #[serde(rename = "canPublish", skip_serializing_if = "Option::is_none")]
    can_publish: Option<bool>,
    #[serde(rename = "canPublishData", skip_serializing_if = "Option::is_none")]
    can_publish_data: Option<bool>,
    #[serde(rename = "canPublishSources", skip_serializing_if = "Option::is_none")]
    can_publish_sources: Option<Vec<&'static str>>,
}

#[derive(Serialize)]
struct LiveKitClaims {
    iss: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    sub: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    nbf: u64,
    exp: u64,
    video: VideoGrant,
}

#[derive(Deserialize)]
struct ListParticipantsResponse {
    #[serde(default)]
    participants: Vec<ParticipantEntry>,
}

#[derive(Deserialize)]
struct ParticipantEntry {
    identity: String,
}

struct FailedJoin {
    count: u32,
    since: Instant,
}

struct HostState {
    meta: RoomMeta,
    secret: u32,
    public_ip: Option<String>,
    seen_ips: HashMap<String, String>,
    failed_joins: HashMap<String, FailedJoin>,
}

pub struct HostController {
    livekit: Mutex<LiveKitProcess>,
    api_key: String,
    api_secret: String,
    livekit_url: String,
    http: Client,
    state: Mutex<HostState>,
}

impl HostController {
    pub fn new(paths: LiveKitPaths) -> Self {
        let lk = LiveKitProcess::new(paths);
        let api_key = lk.api_key.clone();
        let api_secret = lk.api_secret.clone();
        let livekit_url = lk.url();
        let default_meta = RoomMeta {
            v: 1,
            epoch: 0,
            host_id: String::new(),
            host_name: String::new(),
            code: String::new(),
            scope: RoomScope::Lan,
            queue: Vec::new(),
            max_quality: None,
        };
        Self {
            livekit: Mutex::new(lk),
            api_key,
            api_secret,
            livekit_url,
            http: Client::new(),
            state: Mutex::new(HostState {
                meta: default_meta,
                secret: 0,
                public_ip: None,
                seen_ips: HashMap::new(),
                failed_joins: HashMap::new(),
            }),
        }
    }

    pub async fn room_meta(&self) -> RoomMeta {
        self.state.lock().await.meta.clone()
    }

    pub async fn start(&self, opts: HostStartOptions) -> Result<RoomMeta, HostError> {
        {
            let mut lk = self.livekit.lock().await;
            lk.start(opts.node_ip.as_deref()).await?;
        }
        let meta = RoomMeta {
            v: 1,
            epoch: opts.epoch,
            host_id: opts.host_id,
            host_name: opts.host_name,
            code: opts.code,
            scope: if opts.public_ip.is_some() {
                RoomScope::Internet
            } else {
                RoomScope::Lan
            },
            queue: Vec::new(),
            max_quality: None,
        };
        {
            let mut st = self.state.lock().await;
            st.secret = opts.secret;
            st.public_ip = opts.public_ip;
            st.meta = meta.clone();
        }
        let metadata_json = serde_json::to_string(&meta).unwrap_or_default();
        self.twirp_call(
            "CreateRoom",
            &json!({
                "name": ROOM_NAME,
                "max_participants": MAX_PARTICIPANTS,
                "empty_timeout": 600,
                "departure_timeout": 20,
                "metadata": metadata_json,
            }),
        )
        .await?;
        Ok(meta)
    }

    pub async fn stop(&self) {
        self.livekit.lock().await.stop();
    }

    pub fn mint_token(&self, identity: &str, name: &str) -> Result<String, HostError> {
        let now = unix_now();
        let claims = LiveKitClaims {
            iss: self.api_key.clone(),
            sub: Some(identity.to_string()),
            name: Some(name.to_string()),
            nbf: now,
            exp: now + 12 * 3600,
            video: VideoGrant {
                room: ROOM_NAME.to_string(),
                room_join: Some(true),
                room_create: None,
                room_admin: None,
                can_subscribe: Some(true),
                can_publish: Some(true),
                can_publish_data: Some(true),
                can_publish_sources: Some(vec!["screen_share", "screen_share_audio"]),
            },
        };
        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(self.api_secret.as_bytes()),
        )?;
        Ok(token)
    }

    fn mint_admin_token(&self) -> Result<String, HostError> {
        let now = unix_now();
        let claims = LiveKitClaims {
            iss: self.api_key.clone(),
            sub: None,
            name: None,
            nbf: now,
            exp: now + 300,
            video: VideoGrant {
                room: ROOM_NAME.to_string(),
                room_join: None,
                room_create: Some(true),
                room_admin: Some(true),
                can_subscribe: None,
                can_publish: None,
                can_publish_data: None,
                can_publish_sources: None,
            },
        };
        Ok(encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(self.api_secret.as_bytes()),
        )?)
    }

    async fn twirp_call(&self, method: &str, payload: &Value) -> Result<Value, HostError> {
        let auth = self.mint_admin_token()?;
        let url = format!("{}/twirp/livekit.RoomService/{method}", self.livekit_url);
        let res = self
            .http
            .post(url)
            .bearer_auth(auth)
            .json(payload)
            .timeout(Duration::from_secs(5))
            .send()
            .await?;
        let status = res.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(HostError::RoomServiceStatus(status));
        }
        Ok(res.json::<Value>().await.unwrap_or(Value::Null))
    }

    /// POST /api/join vindo de um amigo.
    pub async fn handle_join(&self, req: JoinRequest, remote_ip: &str) -> HttpReply {
        let code_for_reply: String;
        {
            let mut st = self.state.lock().await;
            if let Some(entry) = st.failed_joins.get(remote_ip)
                && entry.since.elapsed() > FAILED_JOIN_WINDOW
            {
                st.failed_joins.remove(remote_ip);
            }
            if st
                .failed_joins
                .get(remote_ip)
                .is_some_and(|e| e.count >= MAX_FAILED_JOINS)
            {
                return HttpReply {
                    status: 429,
                    body: json!({ "error": "too-many-attempts" }),
                };
            }
            let secret_ok = req
                .secret
                .is_some_and(|s| bool::from(s.to_le_bytes().ct_eq(&st.secret.to_le_bytes())));
            if !secret_ok {
                let entry = st
                    .failed_joins
                    .entry(remote_ip.to_string())
                    .or_insert_with(|| FailedJoin {
                        count: 0,
                        since: Instant::now(),
                    });
                entry.count += 1;
                return HttpReply {
                    status: 403,
                    body: json!({ "error": "wrong-code" }),
                };
            }
            code_for_reply = st.meta.code.clone();
        }

        let (Some(raw_id), Some(raw_name)) = (req.identity, req.name) else {
            return HttpReply {
                status: 400,
                body: json!({ "error": "bad-request" }),
            };
        };
        let identity: String = raw_id.chars().take(64).collect();
        let name: String = raw_name.trim().chars().take(32).collect();
        if identity.is_empty() || name.is_empty() {
            return HttpReply {
                status: 400,
                body: json!({ "error": "bad-request" }),
            };
        }

        if let Ok(val) = self
            .twirp_call("ListParticipants", &json!({ "room": ROOM_NAME }))
            .await
            && let Ok(parsed) = serde_json::from_value::<ListParticipantsResponse>(val)
            && parsed.participants.len() >= MAX_PARTICIPANTS
            && !parsed.participants.iter().any(|p| p.identity == identity)
        {
            return HttpReply {
                status: 409,
                body: json!({ "error": "room-full" }),
            };
        }

        self.state
            .lock()
            .await
            .seen_ips
            .insert(identity.clone(), remote_ip.to_string());

        match self.mint_token(&identity, &name) {
            Ok(token) => HttpReply {
                status: 200,
                body: json!({
                    "token": token,
                    "code": code_for_reply,
                    "yourIp": remote_ip,
                }),
            },
            Err(_) => HttpReply {
                status: 500,
                body: json!({ "error": "token-error" }),
            },
        }
    }

    /// O participante abriu as portas dele; testa se ele pode virar host.
    pub async fn participant_ready(
        &self,
        identity: &str,
        reported_ip: Option<&str>,
        listening: bool,
    ) {
        let (target, public_ip, scope) = {
            let st = self.state.lock().await;
            let target = reported_ip
                .map(ToString::to_string)
                .or_else(|| st.seen_ips.get(identity).cloned());
            (target, st.public_ip.clone(), st.meta.scope)
        };

        let mut can_host = false;
        if listening
            && let Some(ref ip) = target
            && public_ip.as_deref() != Some(ip.as_str())
        {
            let reachable_scope = scope == RoomScope::Lan || !is_private_ip(ip);
            if reachable_scope {
                can_host = probe_candidate(ip, identity).await;
            }
        }

        let update_ok = self
            .twirp_call(
                "UpdateParticipant",
                &json!({
                    "room": ROOM_NAME,
                    "identity": identity,
                    "attributes": {
                        "canHost": if can_host { "1" } else { "0" },
                        "endpoint": target.unwrap_or_default(),
                    }
                }),
            )
            .await
            .is_ok();

        if !update_ok {
            return;
        }

        if can_host {
            let meta_json = {
                let mut st = self.state.lock().await;
                if st.meta.queue.iter().any(|id| id == identity) {
                    return;
                }
                st.meta.queue.push(identity.to_string());
                serde_json::to_string(&st.meta).ok()
            };
            if let Some(metadata) = meta_json {
                let _ = self
                    .twirp_call(
                        "UpdateRoomMetadata",
                        &json!({ "room": ROOM_NAME, "metadata": metadata }),
                    )
                    .await;
            }
        }
    }

    pub async fn participant_left(&self, identity: &str) {
        let meta_json = {
            let mut st = self.state.lock().await;
            st.seen_ips.remove(identity);
            let before = st.meta.queue.len();
            st.meta.queue.retain(|id| id != identity);
            if st.meta.queue.len() == before {
                return;
            }
            serde_json::to_string(&st.meta).ok()
        };
        if let Some(metadata) = meta_json {
            let _ = self
                .twirp_call(
                    "UpdateRoomMetadata",
                    &json!({ "room": ROOM_NAME, "metadata": metadata }),
                )
                .await;
        }
    }

    pub async fn set_max_quality(&self, quality: Option<QualityId>) {
        let meta_json = {
            let mut st = self.state.lock().await;
            st.meta.max_quality = quality;
            serde_json::to_string(&st.meta).ok()
        };
        if let Some(metadata) = meta_json {
            let _ = self
                .twirp_call(
                    "UpdateRoomMetadata",
                    &json!({ "room": ROOM_NAME, "metadata": metadata }),
                )
                .await;
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn handle_join_rejects_wrong_secret_and_rate_limits() {
        let ctl = HostController::new(LiveKitPaths {
            exe: PathBuf::from("nonexistent"),
            work_dir: PathBuf::from("."),
        });
        {
            let mut st = ctl.state.lock().await;
            st.secret = 4242;
            st.meta.code = "TEST-CODE-1234".into();
        }

        for _ in 0..MAX_FAILED_JOINS {
            let rep = ctl
                .handle_join(
                    JoinRequest {
                        secret: Some(9999),
                        identity: Some("u1".into()),
                        name: Some("Alice".into()),
                    },
                    "1.2.3.4",
                )
                .await;
            assert_eq!(rep.status, 403);
            assert_eq!(rep.body["error"], "wrong-code");
        }

        let blocked = ctl
            .handle_join(
                JoinRequest {
                    secret: Some(4242),
                    identity: Some("u1".into()),
                    name: Some("Alice".into()),
                },
                "1.2.3.4",
            )
            .await;
        assert_eq!(blocked.status, 429);
        assert_eq!(blocked.body["error"], "too-many-attempts");
    }

    #[test]
    fn mints_valid_livekit_jwt() {
        let ctl = HostController::new(LiveKitPaths {
            exe: PathBuf::from("nonexistent"),
            work_dir: PathBuf::from("."),
        });
        let jwt = ctl.mint_token("client-1", "Maria").unwrap();
        assert_eq!(jwt.split('.').count(), 3);
    }
}
