# Proof P8 — Document Done latch (007 docx forensics)

**Spec:** [007-session-forensics-docx-83c60363](../007-session-forensics-docx-83c60363.md)

## Gates

| Gate | Pass condition | Test / check |
|------|----------------|--------------|
| Class | Shell-built `.docx` → Document | `task_class::shell_landed_docx_is_document` |
| Storm | CodeChange advisory has no browser_navigate recipe | `harness_advisory` CodeChange storm copy |
| Latch | Document + artifact ⇒ mid-loop Done | `document_done_latch` / turn_epilogue |
| Reopen | No “do not stop yet” when Document evidence present | `should_reopen_loop` Document gate |
| TCC | `screencapture` → typed capability_denied | `macos_permissions` / command_interaction |
| Halt | Repeated identical TCC fails → Halt | guardrail / dispatch policy |

## Commands

```bash
cargo test -p edgecrab-core --lib task_class
cargo test -p edgecrab-core --lib document_done
cargo test -p edgecrab-core --lib should_reopen
cargo test -p edgecrab-tools --lib screencapture
bash scripts/check-no-flaky-heuristics.sh
```
