mod client;
mod command;

pub use client::{GatewayClient, HttpGatewayClient};
pub use command::{run_with_client, CliArgs, CliCommand, CliError};

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use crate::{run_with_client, CliArgs, CliCommand, CliError, GatewayClient};

    #[derive(Default)]
    struct MockClient {
        healthz_response: Option<Value>,
        info_response: Option<Value>,
        rpc_response: Option<Value>,
    }

    impl GatewayClient for MockClient {
        fn healthz(&self) -> Result<Value, CliError> {
            self.healthz_response
                .clone()
                .ok_or_else(|| CliError::Transport("healthz response fixture missing".to_owned()))
        }

        fn info(&self) -> Result<Value, CliError> {
            self.info_response
                .clone()
                .ok_or_else(|| CliError::Transport("info response fixture missing".to_owned()))
        }

        fn rpc(&self, _method: &str, _params: Value) -> Result<Value, CliError> {
            self.rpc_response
                .clone()
                .ok_or_else(|| CliError::Transport("rpc response fixture missing".to_owned()))
        }
    }

    #[test]
    fn health_command_requires_ok_true() {
        let args = CliArgs {
            server: "http://127.0.0.1:18789".to_owned(),
            auth_token: None,
            auth_password: None,
            json: false,
            command: CliCommand::Health,
        };

        let client = MockClient {
            healthz_response: Some(json!({ "ok": false })),
            info_response: None,
            rpc_response: None,
        };

        let result = run_with_client(&args, &client);
        assert!(matches!(result, Err(CliError::Protocol(_))));
    }

    #[test]
    fn info_command_returns_payload() {
        let args = CliArgs {
            server: "http://127.0.0.1:18789".to_owned(),
            auth_token: None,
            auth_password: None,
            json: true,
            command: CliCommand::Info,
        };

        let client = MockClient {
            healthz_response: None,
            info_response: Some(json!({ "runtime": "reclaw-core" })),
            rpc_response: None,
        };

        let output = run_with_client(&args, &client).expect("info command should succeed");
        assert_eq!(output["runtime"], "reclaw-core");
    }

    #[test]
    fn rpc_command_rejects_non_object_params() {
        let args = CliArgs {
            server: "http://127.0.0.1:18789".to_owned(),
            auth_token: None,
            auth_password: None,
            json: true,
            command: CliCommand::Rpc {
                method: "system.healthz".to_owned(),
                params: "[]".to_owned(),
            },
        };

        let client = MockClient {
            healthz_response: None,
            info_response: None,
            rpc_response: Some(json!({ "result": {} })),
        };

        let result = run_with_client(&args, &client);
        assert!(matches!(result, Err(CliError::InvalidParams(_))));
    }
}
