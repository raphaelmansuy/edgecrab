# Research / Training Gap Analysis

## Bottom line

Hermes still leads overall on shipped research and training tooling.

It ships concrete research-facing assets that EdgeCrab does not yet match:

- `batch_runner.py`
- `mini_swe_runner.py`
- `trajectory_compressor.py`
- `tools/rl_training_tool.py`
- `tinker-atropos/`

From first principles, these assets matter because research systems need repeatable rollout generation, compression, evaluation, and training loops. A strong personal runtime is not the same thing as a strong research platform.

## Where EdgeCrab exceeds

EdgeCrab exceeds Hermes on one narrower layer: typed trajectory substrate and in-runtime capture wiring.

That advantage is real, but limited:

- `crates/edgecrab-types/src/trajectory.rs` defines a typed trajectory model and JSONL append path
- `crates/edgecrab-core/src/conversation.rs` saves trajectories directly from the live conversation loop
- successful and failed runs are separated at write time

From first principles, this is valuable because the best trajectory system is the one wired into the runtime where the events are born, not bolted on later through lossy post-processing.

## Where Hermes still leads

Hermes still owns the larger research stack because it already provides:

- batch execution for repeated evaluation
- SWE-style task running
- trajectory compression
- RL-training integration
- Atropos-related environment tooling

That is the decisive difference today. EdgeCrab has the typed substrate; Hermes has the broader loop around it.

## Gap verdict

EdgeCrab does not need to rebuild its trajectory layer first. That layer already exists. The missing step is to build the outer research loop around the existing typed capture path:

1. batch evaluation runner
2. trajectory post-processing and compression
3. explicit training and RL interfaces
4. reproducible benchmark workflows across execution backends

## Sources audited

- `edgecrab/crates/edgecrab-types/src/trajectory.rs`
- `edgecrab/crates/edgecrab-core/src/conversation.rs`
- `hermes-agent/batch_runner.py`
- `hermes-agent/mini_swe_runner.py`
- `hermes-agent/trajectory_compressor.py`
- `hermes-agent/tools/rl_training_tool.py`
- `hermes-agent/tinker-atropos/`
