use std::net::TcpStream;

use reqwest::{blocking::Client, StatusCode};
use serde_json::{json, Value};
use tungstenite::{connect, stream::MaybeTlsStream, Message, WebSocket};

use crate::CliError;

const PROTOCOL_VERSION: u64 = 3;
const CONNECT_REQUEST_ID: &str = "connect-1";
const RPC_REQUEST_ID: &str = "rpc-1";

type WsSocket = WebSocket<MaybeTlsStream<TcpStream>>;

pub trait GatewayClient {
    fn healthz(&self) -> Result<Value, CliError>;
    fn info(&self) -> Result<Value, CliError>;
    fn rpc(&self, method: &str, params: Value) -> Result<Value, CliError>;
}

pub struct HttpGatewayClient {
    base_url: String,
    auth_token: Option<String>,
    auth_password: Option<String>,
    client: Client,
}

impl HttpGatewayClient {
    pub fn new(base_url: impl Into<String>) -> Result<Self, CliError> {
        Self::new_with_auth(base_url, None, None)
    }

    pub fn new_with_auth(
        base_url: impl Into<String>,
        auth_token: Option<String>,
        auth_password: Option<String>,
    ) -> Result<Self, CliError> {
        let base_url = normalize_base_url(base_url.into())?;
        let auth_token = normalize_optional_secret(auth_token);
        let auth_password = normalize_optional_secret(auth_password);
        if auth_token.is_some() && auth_password.is_some() {
            return Err(CliError::InvalidAuth(
                "provide only one of --auth-token or --auth-password".to_owned(),
            ));
        }

        let client = Client::builder()
            .build()
            .map_err(|error| CliError::Transport(error.to_string()))?;

        Ok(Self {
            base_url,
            auth_token,
            auth_password,
            client,
        })
    }

    fn get(&self, path: &str) -> Result<Value, CliError> {
        let path = normalize_path(path);
        let url = format!("{}{}", self.base_url, path);

        let response = self
            .client
            .get(&url)
            .send()
            .map_err(|error| CliError::Transport(error.to_string()))?;

        if response.status() != StatusCode::OK {
            return Err(CliError::Protocol(format!(
                "unexpected status {} for GET {path}",
                response.status()
            )));
        }

        response
            .json::<Value>()
            .map_err(|error| CliError::Protocol(error.to_string()))
    }

    fn post_rpc(&self, method: &str, params: Value) -> Result<Value, CliError> {
        let ws_url = websocket_url(&self.base_url);
        let (mut socket, _) = connect(ws_url.as_str())
            .map_err(|error| CliError::Transport(format!("websocket connect failed: {error}")))?;

        let auth = match (&self.auth_token, &self.auth_password) {
            (Some(token), None) => json!({ "token": token }),
            (None, Some(password)) => json!({ "password": password }),
            _ => Value::Null,
        };

        send_json(
            &mut socket,
            &json!({
                "type": "req",
                "id": CONNECT_REQUEST_ID,
                "method": "connect",
                "params": {
                    "minProtocol": PROTOCOL_VERSION,
                    "maxProtocol": PROTOCOL_VERSION,
                    "role": "operator",
                    "client": {
                        "id": "reclaw-cli",
                        "version": env!("CARGO_PKG_VERSION"),
                        "platform": "cli",
                        "mode": "operator"
                    },
                    "auth": auth,
                }
            }),
        )?;
        let _ = read_response_payload(&mut socket, CONNECT_REQUEST_ID)?;

        send_json(
            &mut socket,
            &json!({
                "type": "req",
                "id": RPC_REQUEST_ID,
                "method": method,
                "params": params,
            }),
        )?;
        let payload = read_response_payload(&mut socket, RPC_REQUEST_ID)?;

        let _ = socket.close(None);
        Ok(payload)
    }
}

impl GatewayClient for HttpGatewayClient {
    fn healthz(&self) -> Result<Value, CliError> {
        self.get("/healthz")
    }

    fn info(&self) -> Result<Value, CliError> {
        self.get("/info")
    }

    fn rpc(&self, method: &str, params: Value) -> Result<Value, CliError> {
        self.post_rpc(method, params)
    }
}

fn send_json(socket: &mut WsSocket, payload: &Value) -> Result<(), CliError> {
    let encoded = serde_json::to_string(payload).map_err(|error| {
        CliError::Protocol(format!("failed to encode websocket frame: {error}"))
    })?;
    socket
        .send(Message::Text(encoded.into()))
        .map_err(|error| CliError::Transport(format!("websocket send failed: {error}")))
}

fn read_response_payload(socket: &mut WsSocket, expected_id: &str) -> Result<Value, CliError> {
    loop {
        let frame = read_json_frame(socket)?;

        if frame.get("type").and_then(Value::as_str) != Some("res") {
            continue;
        }

        if frame.get("id").and_then(Value::as_str) != Some(expected_id) {
            continue;
        }

        if frame.get("ok").and_then(Value::as_bool).unwrap_or(false) {
            return Ok(frame.get("payload").cloned().unwrap_or(Value::Null));
        }

        let message = frame
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("rpc request failed");
        return Err(CliError::Protocol(message.to_owned()));
    }
}

fn read_json_frame(socket: &mut WsSocket) -> Result<Value, CliError> {
    loop {
        let message = socket
            .read()
            .map_err(|error| CliError::Transport(format!("websocket read failed: {error}")))?;

        match message {
            Message::Text(text) => {
                return serde_json::from_str(text.as_ref()).map_err(|error| {
                    CliError::Protocol(format!("invalid websocket frame JSON: {error}"))
                });
            }
            Message::Binary(_) => {
                return Err(CliError::Protocol(
                    "unexpected binary websocket frame".to_owned(),
                ));
            }
            Message::Ping(payload) => {
                socket.send(Message::Pong(payload)).map_err(|error| {
                    CliError::Transport(format!("websocket pong failed: {error}"))
                })?;
            }
            Message::Pong(_) => continue,
            Message::Close(_) => {
                return Err(CliError::Protocol(
                    "websocket closed before response".to_owned(),
                ));
            }
            Message::Frame(_) => continue,
        }
    }
}

fn normalize_base_url(input: String) -> Result<String, CliError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(CliError::InvalidServer(
            "server URL cannot be empty".to_owned(),
        ));
    }

    let without_trailing = trimmed.trim_end_matches('/').to_owned();
    if !(without_trailing.starts_with("http://") || without_trailing.starts_with("https://")) {
        return Err(CliError::InvalidServer(
            "server URL must start with http:// or https://".to_owned(),
        ));
    }

    Ok(without_trailing)
}

fn normalize_optional_secret(value: Option<String>) -> Option<String> {
    value.and_then(|raw| {
        let normalized = raw.trim();
        if normalized.is_empty() {
            None
        } else {
            Some(normalized.to_owned())
        }
    })
}

fn normalize_path(path: &str) -> String {
    if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/{path}")
    }
}

fn websocket_url(base_url: &str) -> String {
    if let Some(host) = base_url.strip_prefix("http://") {
        format!("ws://{host}/ws")
    } else if let Some(host) = base_url.strip_prefix("https://") {
        format!("wss://{host}/ws")
    } else {
        format!("{base_url}/ws")
    }
}

#[cfg(test)]
mod tests {
    use std::{net::TcpListener, thread};

    use serde_json::{json, Value};
    use tungstenite::{accept, Message};

    use crate::{
        client::{normalize_base_url, normalize_optional_secret, websocket_url, HttpGatewayClient},
        CliError, GatewayClient,
    };

    #[test]
    fn normalize_base_url_rejects_empty_input() {
        let result = normalize_base_url("   ".to_owned());
        assert!(result.is_err());
    }

    #[test]
    fn normalize_base_url_rejects_invalid_scheme() {
        let result = normalize_base_url("ws://localhost".to_owned());
        assert!(result.is_err());
    }

    #[test]
    fn websocket_url_maps_http_to_ws() {
        assert_eq!(
            websocket_url("http://127.0.0.1:18789"),
            "ws://127.0.0.1:18789/ws"
        );
        assert_eq!(websocket_url("https://example.com"), "wss://example.com/ws");
    }

    #[test]
    fn normalize_optional_secret_trims_and_drops_blank_values() {
        assert_eq!(
            normalize_optional_secret(Some("  token-123  ".to_owned())).as_deref(),
            Some("token-123")
        );
        assert!(normalize_optional_secret(Some("   ".to_owned())).is_none());
        assert!(normalize_optional_secret(None).is_none());
    }

    #[test]
    fn constructor_rejects_token_and_password_together() {
        let result = HttpGatewayClient::new_with_auth(
            "http://127.0.0.1:18789",
            Some("token-a".to_owned()),
            Some("password-b".to_owned()),
        );

        assert!(matches!(result, Err(CliError::InvalidAuth(_))));
    }

    #[test]
    fn rpc_uses_websocket_handshake_and_returns_payload() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let addr = listener
            .local_addr()
            .expect("listener should expose local addr");

        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("connection should arrive");
            let mut ws = accept(stream).expect("websocket handshake should succeed");

            let connect_frame = read_frame(&mut ws);
            assert_eq!(connect_frame["method"], "connect");
            ws.send(Message::Text(
                json!({
                    "type": "res",
                    "id": "connect-1",
                    "ok": true,
                    "payload": { "type": "hello-ok" }
                })
                .to_string()
                .into(),
            ))
            .expect("connect response should be sent");

            let rpc_frame = read_frame(&mut ws);
            assert_eq!(rpc_frame["method"], "health");
            assert_eq!(rpc_frame["params"], json!({"scope":"cli"}));
            ws.send(Message::Text(
                json!({
                    "type": "res",
                    "id": "rpc-1",
                    "ok": true,
                    "payload": { "ok": true }
                })
                .to_string()
                .into(),
            ))
            .expect("rpc response should be sent");
        });

        let client = HttpGatewayClient::new(format!("http://{addr}")).expect("client should build");
        let result = client
            .rpc("health", json!({"scope":"cli"}))
            .expect("rpc should succeed");
        assert_eq!(result["ok"], true);

        let _ = server.join();
    }

    #[test]
    fn rpc_returns_protocol_error_from_gateway_frame() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let addr = listener
            .local_addr()
            .expect("listener should expose local addr");

        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("connection should arrive");
            let mut ws = accept(stream).expect("websocket handshake should succeed");

            let _ = read_frame(&mut ws);
            ws.send(Message::Text(
                json!({
                    "type": "res",
                    "id": "connect-1",
                    "ok": true,
                    "payload": { "type": "hello-ok" }
                })
                .to_string()
                .into(),
            ))
            .expect("connect response should be sent");

            let _ = read_frame(&mut ws);
            ws.send(Message::Text(
                json!({
                    "type": "res",
                    "id": "rpc-1",
                    "ok": false,
                    "error": { "code": "INVALID_REQUEST", "message": "bad params" }
                })
                .to_string()
                .into(),
            ))
            .expect("rpc error response should be sent");
        });

        let client = HttpGatewayClient::new(format!("http://{addr}")).expect("client should build");
        let result = client.rpc("health", json!({}));

        match result {
            Err(CliError::Protocol(message)) => assert!(message.contains("bad params")),
            other => panic!("expected protocol error, got {other:?}"),
        }

        let _ = server.join();
    }

    #[test]
    fn rpc_connect_frame_includes_token_auth_when_configured() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let addr = listener
            .local_addr()
            .expect("listener should expose local addr");

        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("connection should arrive");
            let mut ws = accept(stream).expect("websocket handshake should succeed");

            let connect_frame = read_frame(&mut ws);
            assert_eq!(connect_frame["method"], "connect");
            assert_eq!(connect_frame["params"]["auth"]["token"], "token-123");

            ws.send(Message::Text(
                json!({
                    "type": "res",
                    "id": "connect-1",
                    "ok": true,
                    "payload": { "type": "hello-ok" }
                })
                .to_string()
                .into(),
            ))
            .expect("connect response should be sent");

            let _ = read_frame(&mut ws);
            ws.send(Message::Text(
                json!({
                    "type": "res",
                    "id": "rpc-1",
                    "ok": true,
                    "payload": { "ok": true }
                })
                .to_string()
                .into(),
            ))
            .expect("rpc response should be sent");
        });

        let client = HttpGatewayClient::new_with_auth(
            format!("http://{addr}"),
            Some("token-123".to_owned()),
            None,
        )
        .expect("client should build");
        let result = client.rpc("health", json!({})).expect("rpc should succeed");
        assert_eq!(result["ok"], true);

        let _ = server.join();
    }

    fn read_frame<S>(socket: &mut tungstenite::WebSocket<S>) -> Value
    where
        S: std::io::Read + std::io::Write,
    {
        let message = socket.read().expect("frame should arrive");
        let text = message.into_text().expect("frame should be text");
        serde_json::from_str(text.as_ref()).expect("frame JSON should parse")
    }
}
