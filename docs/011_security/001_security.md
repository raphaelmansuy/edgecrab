# Security Model

Verified against:
- `crates/edgecrab-security/src/lib.rs`
- `crates/edgecrab-security/src/approval.rs`
- `crates/edgecrab-security/src/command_scan.rs`
- `crates/edgecrab-security/src/path_jail.rs`
- `crates/edgecrab-security/src/url_safety.rs`
- `crates/edgecrab-security/src/injection.rs`
- `crates/edgecrab-security/src/redact.rs`

Security checks are split into reusable primitives so tools and runtime code can compose them instead of reimplementing policy in each module.

## Current modules

- `approval`
- `command_scan`
- `injection`
- `normalize`
- `path_jail`
- `path_policy`
- `redact`
- `url_safety`

## Threat classes covered

- path traversal
- unsafe URL fetches and local-network SSRF
- dangerous shell commands
- prompt injection and hidden Unicode tricks
- secret leakage in output
- approval flow for risky operations

## How the layers fit together

```text
user or model request
  -> normalize input if needed
  -> validate path / URL / command
  -> run tool or network action
  -> redact sensitive output
  -> return safe result to the model or user
```

## Design choices visible in code

- the crate denies `unwrap()` usage
- injection checks are re-exported at the crate root for convenience
- output redaction is a first-class step, not a logging afterthought
- approval is explicit policy, not a hidden side effect inside the terminal tool

## Operational rule

If a tool touches the filesystem, network, shell, or durable memory, it should use the shared security primitives before doing real work.
