# CLI Command Spec

## `health`

- Calls `GET /healthz`
- Expects response payload with `ok == true`
- Fails if `ok` is missing or false

## `info`

- Calls `GET /info`
- Prints response payload

## `rpc`

- Calls `POST /`
- Request envelope:

```json
{
  "id": 1,
  "method": "<method>",
  "params": {}
}
```

- `--params` must parse as JSON object.
