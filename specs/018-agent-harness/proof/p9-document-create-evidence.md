# Proof P9 — Document create-evidence latch (008 style-ref forensics)

**Spec:** [008-session-forensics-pptx-style-ref-da8b08e4](../008-session-forensics-pptx-style-ref-da8b08e4.md)

## Gates

| Gate | Pass condition | Test / check |
|------|----------------|--------------|
| Inspect | `ls` of style-reference `.pptx` ⇒ not Done | `task_class::ls_style_reference_pptx_is_not_done` |
| Find | `find …*.pptx` listing ⇒ not Done | `task_class::inspect_only_find_pptx_is_not_done` |
| Shell | Non-inspect generator + `.docx` stdout ⇒ Done | `task_class::shell_landed_docx_is_document` |
| Write | Successful `write_file` `.pptx` ⇒ Done | `task_class::write_file_pptx_is_done` |
| Fail write | `ok:false` write ⇒ not evidence | `task_class::failed_write_file_pptx_is_not_done` |
| Reopen | ls-only transcript still reopens when Incomplete | `turn_epilogue` ls-only reopen |
| Argv | `command_is_inspect_only` facts | `edgecrab-tools` command_interaction |

## Commands

```bash
cargo test -p edgecrab-tools --lib command_is_inspect_only
cargo test -p edgecrab-core --lib task_class
cargo test -p edgecrab-core --lib should_reopen
bash scripts/check-no-flaky-heuristics.sh
```
