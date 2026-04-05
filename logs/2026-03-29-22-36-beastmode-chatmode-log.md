# Task Log

Actions: Wired top-level agent flags into runtime config and AgentBuilder, added conversation trajectory persistence, implemented a native `web_crawl` tool, exposed it through core/ACP tool lists, updated the repo guide, and added regression/unit coverage.

Decisions: Implemented `web_crawl` as an internal SSRF-guarded crawler in `web.rs` instead of adding a new external dependency; treated it as a safe read-only ACP tool alongside `web_search` and `web_extract`; validated with full touched-crate test suites because live Copilot E2E was unavailable in this shell.

Next steps: If you want live provider validation, run `cargo test -p edgecrab-core --test e2e_copilot -- --include-ignored` from a shell that has `VSCODE_IPC_HOOK_CLI` or `VSCODE_COPILOT_TOKEN` set.

Lessons/insights: The real parity bug was not just missing crawl support but previously ignored top-level flags (`save_trajectories`, `skip_context_files`, `skip_memory`); smaller surgical patches were safer than a single large multi-file edit once test contexts drifted.