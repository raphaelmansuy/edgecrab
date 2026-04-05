# Security / Distribution Gap Analysis

## Bottom line

This is still one of the strongest EdgeCrab advantages, but it needs one important caveat stated explicitly: the core CLI is binary-first, while a few optional integrations still pull external runtimes.

## Audited facts

EdgeCrab has a dedicated security crate with explicit modules for:

- approval
- command scanning
- injection detection
- normalization
- path jail
- redaction
- URL safety

EdgeCrab's README also exposes a broader binary-oriented installation surface:

- `cargo install edgecrab-cli`
- `npm install -g edgecrab-cli`
- `pip install edgecrab-cli`
- Docker image usage

Hermes, by contrast, still presents a Python-environment-centric install story in its README.

## Where EdgeCrab exceeds

### 1. Security is a first-class subsystem, not only a collection of helper modules

EdgeCrab makes security a named crate boundary:

- `crates/edgecrab-security`

That is a meaningful architectural difference. It means the security model is visible in the workspace topology, not only implicit in scattered helpers.

From first principles, that improves:

- auditability
- ownership clarity
- reuse across tools and backends
- chances of consistent policy enforcement

### 2. Distribution is materially more operator-friendly

EdgeCrab is easier to ship across common operator preferences:

- Rust-native installation
- npm wrapper
- pip wrapper
- container image
- release binary archives

That broadens who can adopt the system without first becoming a Python environment maintainer.

### 3. The default packaging story is simpler

For the core CLI path, EdgeCrab is much closer to "download binary and run" than Hermes is.

That is a real operational advantage for:

- reproducible installs
- low-friction upgrades
- air-gapped or tightly controlled hosts
- users who do not want Python virtualenv management

## Where Hermes still leads

Hermes still has one real advantage here: more historical operational mileage.

Also, EdgeCrab's binary-first story is not universal across every optional feature. The WhatsApp bridge still requires Node.js and npm, so the strongest version of the distribution claim applies to the core runtime, not to every optional integration path.

## Gap verdict

EdgeCrab should continue treating security and distribution as part of its project identity. The correct refinement is not to weaken that claim, but to state it precisely:

- core runtime and packaging: EdgeCrab ahead
- optional integration dependencies: some caveats remain
- historical operating maturity: Hermes still older

## Sources audited

- `edgecrab/crates/edgecrab-security/src/lib.rs`
- `edgecrab/crates/edgecrab-cli/src/gateway_setup.rs`
- `edgecrab/crates/edgecrab-cli/src/whatsapp_cmd.rs`
- `edgecrab/README.md`
- `hermes-agent/tools/approval.py`
- `hermes-agent/tools/tirith_security.py`
- `hermes-agent/gateway/pairing.py`
- `hermes-agent/README.md`
