//! Servidor na porta pública 7880:
//!  - todos:     GET /probe e eco UDP na 7882
//!  - só o host: POST /api/join e proxy (HTTP e WebSocket) para o LiveKit em 127.0.0.1:7890

use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::ws::{Message as AxumWsMsg, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Query, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::{Json, Router};
use ct_core::protocol::{LIVEKIT_INTERNAL_PORT, PUBLIC_PORT};
use ct_net::ip::normalize_remote_ip;
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use serde_json::json;
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::TcpListener;
use tokio::sync::{RwLock, oneshot};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message as TungMsg;

use crate::host_controller::{HostController, JoinRequest};
use crate::probe::UdpProbeResponder;

#[derive(Clone)]
struct ServerState {
    identity: String,
    host: Arc<RwLock<Option<Arc<HostController>>>>,
    http: Client,
}

pub struct PublicServer {
    identity: String,
    host: Arc<RwLock<Option<Arc<HostController>>>>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    server_task: Option<JoinHandle<()>>,
    udp_probe: Option<UdpProbeResponder>,
}

impl PublicServer {
    pub fn new(identity: impl Into<String>) -> Self {
        Self {
            identity: identity.into(),
            host: Arc::new(RwLock::new(None)),
            shutdown_tx: None,
            server_task: None,
            udp_probe: None,
        }
    }

    pub async fn set_host(&self, host: Option<Arc<HostController>>) {
        *self.host.write().await = host;
    }

    pub async fn listen(&mut self) -> bool {
        if self.server_task.is_some() {
            return true;
        }
        let Some(listener) = bind_tcp_dual_stack(PUBLIC_PORT).or_else(|| bind_tcp_v4(PUBLIC_PORT))
        else {
            return false;
        };

        let state = ServerState {
            identity: self.identity.clone(),
            host: self.host.clone(),
            http: Client::new(),
        };

        let app = Router::new()
            .route("/probe", get(handle_probe))
            .route("/api/join", post(handle_join))
            .fallback(any(handle_proxy))
            .with_state(state);

        let (tx, rx) = oneshot::channel();
        self.shutdown_tx = Some(tx);
        self.server_task = Some(tokio::spawn(async move {
            let svc = app.into_make_service_with_connect_info::<SocketAddr>();
            let _ = axum::serve(listener, svc)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await;
        }));
        true
    }

    pub fn start_udp_probe(&mut self) -> bool {
        if self.udp_probe.is_some() {
            return true;
        }
        self.udp_probe = UdpProbeResponder::start();
        self.udp_probe.is_some()
    }

    pub fn stop_udp_probe(&mut self) {
        if let Some(p) = self.udp_probe.take() {
            p.stop();
        }
    }

    pub fn close(&mut self) {
        self.stop_udp_probe();
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(task) = self.server_task.take() {
            task.abort();
        }
    }
}

impl Drop for PublicServer {
    fn drop(&mut self) {
        self.close();
    }
}

async fn handle_probe(
    State(state): State<ServerState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let nonce = params.get("nonce").cloned();
    Json(json!({ "nonce": nonce, "id": state.identity }))
}

async fn handle_join(
    State(state): State<ServerState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<JoinRequest>,
) -> Response {
    let host = state.host.read().await.clone();
    let Some(host) = host else {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": "not-host" }))).into_response();
    };
    let remote_ip = normalize_remote_ip(addr.ip());
    let reply = host.handle_join(req, &remote_ip).await;
    let status = StatusCode::from_u16(reply.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, Json(reply.body)).into_response()
}

async fn handle_proxy(State(state): State<ServerState>, req: Request) -> Response {
    if state.host.read().await.is_none() {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": "not-host" }))).into_response();
    }

    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_else(|| "/".to_string());

    let (mut parts, body) = req.into_parts();
    if let Ok(ws) = <WebSocketUpgrade as axum::extract::FromRequestParts<ServerState>>::from_request_parts(&mut parts, &state).await {
        return ws.on_upgrade(move |client_ws| proxy_ws(client_ws, path_and_query));
    }

    let req = Request::from_parts(parts, body);
    proxy_http(&state.http, req, &path_and_query).await
}

async fn proxy_http(client: &Client, req: Request, path_and_query: &str) -> Response {
    let method = req.method().clone();
    let headers = req.headers().clone();
    let body_bytes = axum::body::to_bytes(req.into_body(), 64 * 1024)
        .await
        .unwrap_or_default();

    let url = format!("http://127.0.0.1:{LIVEKIT_INTERNAL_PORT}{path_and_query}");
    let mut builder = client.request(method, url).body(body_bytes);
    for (k, v) in &headers {
        if k != "host" {
            builder = builder.header(k, v);
        }
    }

    let Ok(up) = builder.send().await else {
        return StatusCode::BAD_GATEWAY.into_response();
    };

    let status = up.status();
    let mut out_headers = HeaderMap::new();
    for (k, v) in up.headers() {
        out_headers.insert(k.clone(), v.clone());
    }
    let bytes = up.bytes().await.unwrap_or_default();
    let mut resp = Response::new(Body::from(bytes));
    *resp.status_mut() = status;
    *resp.headers_mut() = out_headers;
    resp
}

async fn proxy_ws(client_ws: WebSocket, path_and_query: String) {
    let upstream_url = format!("ws://127.0.0.1:{LIVEKIT_INTERNAL_PORT}{path_and_query}");
    let Ok((upstream_ws, _)) = tokio_tungstenite::connect_async(&upstream_url).await else {
        return;
    };

    let (mut client_tx, mut client_rx) = client_ws.split();
    let (mut up_tx, mut up_rx) = upstream_ws.split();

    let c_to_u = async {
        while let Some(Ok(msg)) = client_rx.next().await {
            let converted = match msg {
                AxumWsMsg::Text(t) => TungMsg::Text(t.to_string().into()),
                AxumWsMsg::Binary(b) => TungMsg::Binary(b),
                AxumWsMsg::Ping(p) => TungMsg::Ping(p),
                AxumWsMsg::Pong(p) => TungMsg::Pong(p),
                AxumWsMsg::Close(_) => {
                    let _ = up_tx.send(TungMsg::Close(None)).await;
                    break;
                }
            };
            if up_tx.send(converted).await.is_err() {
                break;
            }
        }
    };

    let u_to_c = async {
        while let Some(Ok(msg)) = up_rx.next().await {
            let converted = match msg {
                TungMsg::Text(t) => AxumWsMsg::Text(t.to_string().into()),
                TungMsg::Binary(b) => AxumWsMsg::Binary(b),
                TungMsg::Ping(p) => AxumWsMsg::Ping(p),
                TungMsg::Pong(p) => AxumWsMsg::Pong(p),
                TungMsg::Close(_) => {
                    let _ = client_tx.send(AxumWsMsg::Close(None)).await;
                    break;
                }
                TungMsg::Frame(_) => continue,
            };
            if client_tx.send(converted).await.is_err() {
                break;
            }
        }
    };

    tokio::select! {
        _ = c_to_u => {}
        _ = u_to_c => {}
    }
}

fn bind_tcp_dual_stack(port: u16) -> Option<TcpListener> {
    let sock = Socket::new(Domain::IPV6, Type::STREAM, Some(Protocol::TCP)).ok()?;
    let _ = sock.set_only_v6(false);
    let _ = sock.set_reuse_address(true);
    sock.set_nonblocking(true).ok()?;
    sock.bind(&SocketAddr::from(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, port, 0, 0)).into())
        .ok()?;
    sock.listen(128).ok()?;
    let std_listener: std::net::TcpListener = sock.into();
    TcpListener::from_std(std_listener).ok()
}

fn bind_tcp_v4(port: u16) -> Option<TcpListener> {
    let sock = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP)).ok()?;
    let _ = sock.set_reuse_address(true);
    sock.set_nonblocking(true).ok()?;
    sock.bind(&SocketAddr::from(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port)).into())
        .ok()?;
    sock.listen(128).ok()?;
    let std_listener: std::net::TcpListener = sock.into();
    TcpListener::from_std(std_listener).ok()
}
