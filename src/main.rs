use std::process::ExitCode;

use clap::Parser;
use reclaw_cli::{run_with_client, CliArgs, HttpGatewayClient};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("reclaw-cli failed: {error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let args = CliArgs::parse();
    let client = HttpGatewayClient::new_with_auth(
        args.server.clone(),
        args.auth_token.clone(),
        args.auth_password.clone(),
    )
    .map_err(|error| error.to_string())?;
    let output = run_with_client(&args, &client).map_err(|error| error.to_string())?;

    if args.json {
        let text = serde_json::to_string_pretty(&output)
            .map_err(|error| format!("failed to encode output as JSON: {error}"))?;
        println!("{text}");
    } else {
        println!("{output}");
    }

    Ok(())
}
