use clap::{Parser, Subcommand};
use serde_json::Value;
use thiserror::Error;

use crate::GatewayClient;

#[derive(Debug, Clone, Parser)]
#[command(name = "reclaw-cli", version)]
pub struct CliArgs {
    #[arg(long, default_value = "http://127.0.0.1:18789")]
    pub server: String,

    #[arg(long)]
    pub auth_token: Option<String>,

    #[arg(long)]
    pub auth_password: Option<String>,

    #[arg(long)]
    pub json: bool,

    #[command(subcommand)]
    pub command: CliCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum CliCommand {
    /// Query /healthz and assert ok=true.
    Health,

    /// Query /info.
    Info,

    /// Invoke a JSON-RPC method over WebSocket RPC.
    Rpc {
        method: String,
        #[arg(long, default_value = "{}")]
        params: String,
    },
}

#[derive(Debug, Error)]
pub enum CliError {
    #[error("invalid server URL: {0}")]
    InvalidServer(String),

    #[error("transport failure: {0}")]
    Transport(String),

    #[error("protocol error: {0}")]
    Protocol(String),

    #[error("invalid rpc params: {0}")]
    InvalidParams(String),

    #[error("invalid auth options: {0}")]
    InvalidAuth(String),
}

pub fn run_with_client(args: &CliArgs, client: &dyn GatewayClient) -> Result<Value, CliError> {
    match &args.command {
        CliCommand::Health => {
            let payload = client.healthz()?;
            let is_ok = payload.get("ok").and_then(Value::as_bool).unwrap_or(false);
            if is_ok {
                Ok(payload)
            } else {
                Err(CliError::Protocol(
                    "healthz response missing ok=true".to_owned(),
                ))
            }
        }
        CliCommand::Info => client.info(),
        CliCommand::Rpc { method, params } => {
            let params = parse_params(params)?;
            client.rpc(method, params)
        }
    }
}

fn parse_params(raw: &str) -> Result<Value, CliError> {
    let parsed: Value =
        serde_json::from_str(raw).map_err(|error| CliError::InvalidParams(error.to_string()))?;

    if parsed.is_object() {
        Ok(parsed)
    } else {
        Err(CliError::InvalidParams(
            "params JSON must be an object".to_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::{run_with_client, CliArgs, CliCommand, CliError, GatewayClient};

    struct StaticClient;

    impl GatewayClient for StaticClient {
        fn healthz(&self) -> Result<serde_json::Value, CliError> {
            Ok(json!({ "ok": true }))
        }

        fn info(&self) -> Result<serde_json::Value, CliError> {
            Ok(json!({ "runtime": "reclaw-core" }))
        }

        fn rpc(
            &self,
            method: &str,
            params: serde_json::Value,
        ) -> Result<serde_json::Value, CliError> {
            Ok(json!({ "method": method, "params": params }))
        }
    }

    #[test]
    fn rpc_command_accepts_object_params() {
        let args = CliArgs {
            server: "http://127.0.0.1:18789".to_owned(),
            auth_token: None,
            auth_password: None,
            json: false,
            command: CliCommand::Rpc {
                method: "system.healthz".to_owned(),
                params: "{\"scope\":\"node\"}".to_owned(),
            },
        };

        let output = run_with_client(&args, &StaticClient).expect("rpc should succeed");
        assert_eq!(output["params"]["scope"], "node");
    }

    #[test]
    fn rpc_command_rejects_invalid_json() {
        let args = CliArgs {
            server: "http://127.0.0.1:18789".to_owned(),
            auth_token: None,
            auth_password: None,
            json: false,
            command: CliCommand::Rpc {
                method: "system.healthz".to_owned(),
                params: "{invalid".to_owned(),
            },
        };

        let result = run_with_client(&args, &StaticClient);
        assert!(matches!(result, Err(CliError::InvalidParams(_))));
    }
}
