# 013 — Verdict & Strategy

Brutal synthesis: what to use, when to migrate, what not to pretend.

---

## TL;DR

**Hermes** and **EdgeCrab** are not competitors in the sense of one dominating the other. They are the **same product philosophy** (ReAct agent + tools + gateway + skills) implemented for **different optimization targets**:

| Optimize for | Choose |
|--------------|--------|
| Plugin ecosystem, provider breadth, multi-agent ops, desktop/dashboard | **Hermes** |
| Single binary, Rust performance, LSP-integrated coding, Hermes migration, SDK | **EdgeCrab** |

EdgeCrab is a **credible Hermes successor for the coding-agent core** — not a full replacement for the entire Hermes plugin surface.

---

## Where Hermes is genuinely ahead

These are not "missing polish" — they are **architectural/product bets** Hermes shipped years ahead:

1. **Kanban + durable multi-agent queue** — operational model for fire-and-forget agent fleets (`kanban.db`, 9 tools). EdgeCrab has delegate UI but no equivalent work queue.

2. **Plugin economy** — 100+ plugins for providers, platforms, memory, web, browser, observability. EdgeCrab's gap **009** is real; compile-time catalogs don't replace pip-install platform adapters.

3. **Provider long tail** — Qwen OAuth, Gemini CLI, Kimi CN, Stepfun, Tencent, credential pools, Codex app-server runtime. EdgeCrab covers the **Western subscription triangle** well; Hermes covers **global integrator long tail**.

4. **Curator + skill governance** — human-in-the-loop skill/memory approvals + background staleness review. EdgeCrab has guard scanning but not curator ops.

5. **Surface area** — Electron desktop, React TUI sidecar, web dashboard, MCP **server** mode, automation blueprints.

6. **Test mass** — ~25k tests vs ~650+. Hermes CI culture is mature for a Python monolith.

**If you depend on any of the above, stay on Hermes** until the specific gap ID in [012-master-gap-matrix.md](012-master-gap-matrix.md) is closed with a `proof/` folder.

---

## Where EdgeCrab is genuinely ahead

1. **Deployability** — one static binary, no Python+Node runtime stack, Termux feature flag.

2. **Coding integrity loop** — LSP 25-tool integration + post-write diagnostics gate + per-turn mutation footer + optional shadow judge. This is a coherent "don't silently break the repo" story Hermes only partially matches.

3. **Mission steering model** — typed Hint/Redirect/Stop with cancel token vs text-only `/steer`.

4. **TUI delegation control plane** — spawn pause, per-agent kill, disk replay, Gantt strip (post spec-002 work). Hermes `/agents` is strong; EC matched and exceeded on **control** dimensions.

5. **SDK** — `edgecrab-sdk-core` + Python/Node bindings for embedding agents in products.

6. **Internal architecture** — 20-crate workspace, clippy `-D warnings`, deny unwrap in prod tools. Lower crash class from type system.

7. **Migration path** — `edgecrab migrate` imports Hermes config, state, memories, skills with compat layer.

**If you want a Rust-native coding agent with gateway**, EdgeCrab is the better default **today**.

---

## Parity zones (don't oversell gaps)

These are **done** — marketing either side as "unique" is dishonest:

- ReAct loop, streaming, interrupt
- Compression + prompt cache discipline
- Persistent goals / Ralph loop
- Cron scheduling
- MCP client (stdio + HTTP OAuth)
- ACP IDE adapter
- Core gateway platforms (Telegram, Discord, Slack, WhatsApp, Signal, email, Matrix, Feishu, WeCom, Weixin, Mattermost, DingTalk, BlueBubbles, webhook, API server)
- Checkpoints / rollback (filesystem)
- Computer use (macOS cua-driver)
- Skills hub + security guard
- Session FTS + branch/background
- Profiles (isolated homes)
- Git worktree sessions
- Subscription proxy (shape differs)
- Honcho memory tools

---

## Migration guidance

### Hermes → EdgeCrab

```bash
edgecrab migrate --dry-run   # preview
edgecrab migrate             # live
```

**Expect:**

| Asset | Result |
|-------|--------|
| config.yaml | Migrated with key aliasing |
| memories/skills | Copied |
| sessions | Imported (dedup IDs) |
| Plugins | **Not migrated** — find EC equivalent or run Hermes alongside |
| Kanban jobs | **Lost** — no EC kanban |
| Custom Python hooks | May work if placed in `~/.edgecrab/hooks/` |

**Run both during transition:** Hermes gateway for Teams plugin; EdgeCrab CLI for local coding — feasible via separate profiles/homes.

### EdgeCrab → Hermes

No first-class tool. Manual copy of `~/.edgecrab/` → `~/.hermes/` with config massage. Rare direction.

---

## Strategic recommendations

### For Nous / maintainers

| Action | Rationale |
|--------|-----------|
| Keep Hermes as **plugin host of record** | EC won't replicate 100 plugins quickly |
| Position EdgeCrab as **performance + coding-agent SKU** | Clear story beats "everything everywhere" |
| Prioritize gap **007 kanban** or document delegate-only ops | Biggest ops feature miss |
| Prioritize gap **009** or officially defer plugins forever | Sets expectations |
| Continue `app.rs` decomposition | Biggest EC engineering risk |

### For users choosing today

| Persona | Recommendation |
|---------|----------------|
| Solo dev, terminal coding | **EdgeCrab** |
| Multi-platform ops (Teams, LINE, ntfy) | **Hermes** |
| Subscription OAuth only (Claude/GPT/Grok/Copilot) | **Either** |
| macOS computer use | **Either** |
| Android Termux | **EdgeCrab** (feature flag) |
| IDE via ACP | **Either** |
| Embedding in product | **EdgeCrab SDK** |
| Long-running agent fleet + kanban | **Hermes** |

---

## Honest weaknesses (both)

| Weakness | Hermes | EdgeCrab |
|----------|--------|----------|
| Monolith file | `tui_gateway/server.py` | `app.rs` |
| UI OOM / crash | Ink verbose trail | Less common |
| Prompt injection → exfil | Mitigated not solved | Same |
| Lossy compression | Yes | Yes |
| "Done" detection | Heuristic | Heuristic + costly shadow judge |

---

## Final grades (product, not morality)

| Product | Grade | One-line |
|---------|-------|----------|
| **Hermes** | **A− ecosystem** | Broadest agent OS; Python+Node tax |
| **EdgeCrab** | **B+ core / C ecosystem** | Best Rust coding agent fork of Hermes; plugins immature |

Combined, they represent **the most complete open agent runtime pair** in this repo — compare them to each other, not to ChatGPT-in-a-browser.

---

## Next documents to write (optional)

- Per-gap deep dives already exist in `001-gap-analysis-v14/`
- TUI-only: `002-tui-hemes-vs-edgecrab/`
- This folder should **not** duplicate those — update [012](012-master-gap-matrix.md) when gaps close.

---

*Assessment date: June 2026. Re-verify after major releases by re-running codebase exploration on both paths.*
