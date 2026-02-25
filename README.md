# Reclaw CLI

`reclaw-cli` is an operational command-line interface for Reclaw Core and OpenClaw-compatible servers.

## Commands

- `health`: query `/healthz`
- `info`: query `/info`
- `rpc`: invoke JSON-RPC method over `POST /`

## Run

```bash
cargo run -- --server http://127.0.0.1:18789 health
cargo run -- --server http://127.0.0.1:18789 info --json
cargo run -- --server http://127.0.0.1:18789 rpc system.healthz --params '{}'
```

## Quality Gates

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```
