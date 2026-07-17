# 032 Secrets Manager — Implementation Proof

Shipped:

- `edgecrab-security::secrets` — `SecretResolver`, env + file backends
- CLI: `edgecrab secret list|get|set`
- File store under `~/.edgecrab/secrets/` with `0o600`
