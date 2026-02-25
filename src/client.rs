use reqwest::{blocking::Client, StatusCode};
use serde_json::{json, Value};

use crate::CliError;

pub trait GatewayClient {
    fn healthz(&self) -> Result<Value, CliError>;
    fn info(&self) -> Result<Value, CliError>;
    fn rpc(&self, method: &str, params: Value) -> Result<Value, CliError>;
}

pub struct HttpGatewayClient {
    base_url: String,
    client: Client,
}

impl HttpGatewayClient {
    pub fn new(base_url: impl Into<String>) -> Result<Self, CliError> {
        let base_url = normalize_base_url(base_url.into())?;
        let client = Client::builder()
            .build()
            .map_err(|error| CliError::Transport(error.to_string()))?;

        Ok(Self { base_url, client })
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
        let body = json!({
            "id": 1,
            "method": method,
            "params": params,
        });

        let response = self
            .client
            .post(&self.base_url)
            .json(&body)
            .send()
            .map_err(|error| CliError::Transport(error.to_string()))?;

        if response.status() != StatusCode::OK {
            return Err(CliError::Protocol(format!(
                "unexpected status {} for POST /",
                response.status()
            )));
        }

        response
            .json::<Value>()
            .map_err(|error| CliError::Protocol(error.to_string()))
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

fn normalize_path(path: &str) -> String {
    if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/{path}")
    }
}

#[cfg(test)]
mod tests {
    use crate::client::normalize_base_url;

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
}
