# EdgeCrab SDK — Real-World Agent Examples

> **Cross-references:** [SPEC](02-SPEC.md) | [TUTORIAL](04-TUTORIAL.md) | [CUSTOM-TOOLS](09-CUSTOM-TOOLS.md)

---

## WHY This Document Exists

Hello-world agents prove nothing. This document contains **10 production-quality agent examples** that demonstrate real value — the kind of agents people actually build, deploy, and pay for. Each example is complete, runnable, and showcases EdgeCrab capabilities no competing SDK can match.

Every example includes:
- **The problem** it solves (who cares, why they care)
- **Complete code** in Python, Node.js, and Rust
- **Which EdgeCrab capabilities** make it possible
- **What competitors can't do** (or require 10x more code)

---

## Example 1: Codebase Reviewer Agent

**Problem:** You push a branch. You want an AI to review the diff against project conventions, find bugs, check test coverage, and post a summary — all before a human looks at it.

**EdgeCrab capabilities used:** `terminal` (git diff), `file_read` (read source), `file_search` (find tests), `web_search` (check docs for libraries used), session persistence (remember project conventions across reviews).

### Python

```python
from edgecrab import Agent, Tool

@Tool("get_git_diff", description="Get the git diff for the current branch against main")
async def get_git_diff(base_branch: str = "main") -> str:
    import subprocess
    result = subprocess.run(
        ["git", "diff", f"{base_branch}...HEAD", "--stat", "--patch"],
        capture_output=True, text=True, timeout=30,
    )
    return result.stdout[:50_000]  # Truncate for context window

@Tool("run_tests", description="Run the test suite and return results")
async def run_tests(test_path: str = "tests/") -> dict:
    import subprocess
    result = subprocess.run(
        ["python", "-m", "pytest", test_path, "-q", "--tb=short"],
        capture_output=True, text=True, timeout=120,
    )
    return {
        "exit_code": result.returncode,
        "stdout": result.stdout[-5000:],
        "stderr": result.stderr[-2000:],
    }

@Tool("post_review_comment", description="Post a review summary to the PR")
async def post_review_comment(summary: str, issues: list = None) -> dict:
    # In production, use GitHub API
    print(f"\n{'='*60}\nREVIEW SUMMARY:\n{summary}\n{'='*60}")
    return {"posted": True, "issues_count": len(issues or [])}


async def review_branch():
    agent = Agent(
        model="anthropic/claude-sonnet-4",
        tools=[get_git_diff, run_tests, post_review_comment],
        instructions="""You are a senior code reviewer. For each review:
1. Get the git diff
2. Read the changed files to understand context
3. Run the test suite
4. Check for: bugs, security issues, missing tests, style violations
5. Post a structured review with severity levels (critical/warning/info)
Be constructive. Suggest specific fixes, not vague complaints.""",
        max_iterations=30,
    )

    result = await agent.run(
        "Review the current branch against main. "
        "Focus on correctness, security, and test coverage."
    )

    print(f"\nReview complete. Cost: ${result.cost.total_cost:.4f}")
    print(f"API calls: {result.api_calls}")
    return result

# Run it
import asyncio
asyncio.run(review_branch())
```

### Node.js

```typescript
import { Agent, Tool } from "edgecrab";
import { execSync } from "child_process";

const getGitDiff = Tool.create({
  name: "get_git_diff",
  description: "Get the git diff for the current branch against main",
  parameters: {
    base_branch: { type: "string", default: "main" },
  },
  handler: async (args) => {
    const diff = execSync(
      `git diff ${args.base_branch}...HEAD --stat --patch`,
      { encoding: "utf-8", timeout: 30_000 }
    );
    return diff.slice(0, 50_000);
  },
});

const runTests = Tool.create({
  name: "run_tests",
  description: "Run the test suite",
  parameters: {
    test_path: { type: "string", default: "tests/" },
  },
  handler: async (args) => {
    try {
      const output = execSync(`npx jest ${args.test_path} --no-coverage`, {
        encoding: "utf-8",
        timeout: 120_000,
      });
      return { exit_code: 0, stdout: output.slice(-5000) };
    } catch (e: any) {
      return {
        exit_code: e.status,
        stdout: e.stdout?.slice(-5000) ?? "",
        stderr: e.stderr?.slice(-2000) ?? "",
      };
    }
  },
});

async function main() {
  const agent = new Agent("anthropic/claude-sonnet-4", {
    tools: [getGitDiff, runTests],
    instructions: `You are a senior code reviewer. Be specific and constructive.`,
    maxIterations: 30,
  });

  const result = await agent.run(
    "Review the current branch. Focus on bugs and security."
  );
  console.log(`Cost: $${result.cost.totalCost.toFixed(4)}`);
}

main();
```

### Rust

```rust
use edgecrab_sdk::prelude::*;
use serde_json::json;
use std::process::Command;

struct GitDiffTool;

#[async_trait]
impl ToolHandler for GitDiffTool {
    fn name(&self) -> &'static str { "get_git_diff" }
    fn toolset(&self) -> &'static str { "review" }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "get_git_diff".into(),
            description: "Get git diff against main".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "base_branch": { "type": "string", "default": "main" }
                }
            }),
            strict: None,
        }
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext)
        -> Result<String, ToolError>
    {
        let base = args["base_branch"].as_str().unwrap_or("main");
        let output = Command::new("git")
            .args(["diff", &format!("{base}...HEAD"), "--stat", "--patch"])
            .current_dir(&ctx.cwd)
            .output()
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "get_git_diff".into(),
                message: e.to_string(),
            })?;
        let diff = String::from_utf8_lossy(&output.stdout);
        Ok(diff[..diff.len().min(50_000)].to_string())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let agent = Agent::new("anthropic/claude-sonnet-4")
        .tool(GitDiffTool)
        .max_iterations(30)
        .build()?;

    let result = agent.run("Review the current branch against main").await?;
    println!("Cost: ${:.4}", result.cost.total_cost);
    Ok(())
}
```

**Why EdgeCrab wins:** Built-in `file_read`, `file_search`, `terminal`, and `web_search` tools mean the agent can autonomously read files, run commands, and research libraries. Competitors require you to implement all of these yourself.

---

## Example 2: Infrastructure Monitor Agent

**Problem:** You want an agent that monitors server health, checks logs for anomalies, and alerts you via Telegram/Slack when something looks wrong — proactively, on a schedule.

**EdgeCrab capabilities used:** `terminal` (run health checks), `web_search` (lookup error codes), `session_search` (recall past incidents), `send_message` (alert via gateway), `cron` (scheduled execution), memory (remember known issues).

### Python

```python
from edgecrab import Agent, Tool
import json

@Tool("check_server_health", description="Run health checks on servers")
async def check_server_health(host: str) -> dict:
    import subprocess
    checks = {}

    # CPU usage
    result = subprocess.run(
        ["ssh", host, "top -bn1 | head -5"],
        capture_output=True, text=True, timeout=10,
    )
    checks["cpu"] = result.stdout

    # Disk usage
    result = subprocess.run(
        ["ssh", host, "df -h | head -10"],
        capture_output=True, text=True, timeout=10,
    )
    checks["disk"] = result.stdout

    # Recent errors in syslog
    result = subprocess.run(
        ["ssh", host, "journalctl -p err --since '1 hour ago' --no-pager | tail -20"],
        capture_output=True, text=True, timeout=10,
    )
    checks["recent_errors"] = result.stdout

    return checks

@Tool("check_docker_containers", description="Check Docker container status")
async def check_docker_containers(host: str) -> dict:
    import subprocess
    result = subprocess.run(
        ["ssh", host, "docker ps --format '{{.Names}} {{.Status}}'"],
        capture_output=True, text=True, timeout=10,
    )
    containers = []
    for line in result.stdout.strip().split("\n"):
        if line:
            parts = line.split(" ", 1)
            containers.append({"name": parts[0], "status": parts[1] if len(parts) > 1 else "unknown"})
    return {"containers": containers, "count": len(containers)}

@Tool("send_alert", description="Send an alert to the ops team")
async def send_alert(severity: str, title: str, details: str) -> dict:
    # In production, use PagerDuty/Slack/Telegram API
    print(f"\n🚨 [{severity.upper()}] {title}\n{details}")
    return {"sent": True, "severity": severity}


async def run_health_check():
    agent = Agent(
        model="anthropic/claude-sonnet-4",
        tools=[check_server_health, check_docker_containers, send_alert],
        session_id="infra-monitor",  # Persistent session — remembers past checks
        instructions="""You are an infrastructure monitoring agent.
Your job:
1. Check all servers (prod-web-1, prod-web-2, prod-db-1)
2. Check Docker containers on each server
3. Compare with previous checks (you have session history)
4. If anything is degraded, send an alert with severity:
   - critical: service down, disk >95%, OOM kills
   - warning: high CPU (>80%), disk >80%, container restarts
   - info: new patterns, configuration drift
5. Always end with a one-paragraph summary""",
        max_iterations=40,
    )

    result = await agent.run(
        "Run a full health check on all production servers. "
        "Compare with our last check and highlight any changes."
    )

    print(f"\nHealth check complete. Cost: ${result.cost.total_cost:.4f}")
    return result
```

**Why EdgeCrab wins:**
- `session_id="infra-monitor"` gives the agent persistent memory of past checks — it can detect degradation trends
- `send_message` tool (via gateway) delivers alerts to 17 platforms
- `cron` tool enables scheduling without external infrastructure
- No competitor SDK has all three: persistent sessions + multi-platform messaging + built-in scheduling

---

## Example 3: Research Assistant Agent

**Problem:** A researcher needs to survey the literature on a topic, extract key findings, identify gaps, and produce a structured report with citations.

**EdgeCrab capabilities used:** `web_search`, `web_extract`, `web_crawl`, `file_write`, `memory` (remember prior research sessions), `session_search` (find previous research on related topics).

### Python

```python
from edgecrab import Agent, Tool
import json

@Tool("save_finding", description="Save a research finding to the knowledge base")
async def save_finding(
    title: str,
    source_url: str,
    key_finding: str,
    relevance: str = "medium",
) -> dict:
    finding = {
        "title": title,
        "source": source_url,
        "finding": key_finding,
        "relevance": relevance,
    }
    # In production, save to a database
    return {"saved": True, "finding": finding}

@Tool("generate_bibliography", description="Generate BibTeX entries from URLs")
async def generate_bibliography(urls: list) -> str:
    entries = []
    for i, url in enumerate(urls):
        entry = f"""@misc{{ref{i+1},
  title = {{Source {i+1}}},
  url = {{{url}}},
  note = {{Accessed 2026}},
}}"""
        entries.append(entry)
    return "\n\n".join(entries)


async def research_topic(topic: str):
    agent = Agent(
        model="anthropic/claude-sonnet-4",
        tools=[save_finding, generate_bibliography],
        # Built-in tools: web_search, web_extract, file_write are already available
        instructions="""You are a research assistant specializing in literature surveys.
For each research task:
1. Search the web for 5-10 relevant sources (academic papers, blog posts, docs)
2. Extract key findings from each source using web_extract
3. Save each finding using save_finding with relevance rating
4. Write a structured report to a markdown file with:
   - Executive summary (3-5 sentences)
   - Key findings (numbered, with citations)
   - Research gaps identified
   - Suggested next steps
5. Generate a bibliography
Be rigorous. Distinguish facts from opinions. Note conflicting findings.""",
        max_iterations=50,
    )

    result = await agent.run(
        f"Conduct a literature survey on: {topic}. "
        f"Write the report to research_report.md."
    )
    print(f"Research complete. Cost: ${result.cost.total_cost:.4f}")
    print(f"Sources checked: {result.api_calls} API calls")

asyncio.run(research_topic("zero-knowledge proofs for AI model verification"))
```

### Node.js

```typescript
import { Agent, Tool } from "edgecrab";

const saveFinding = Tool.create({
  name: "save_finding",
  description: "Save a research finding to the knowledge base",
  parameters: {
    title: { type: "string" },
    source_url: { type: "string" },
    key_finding: { type: "string" },
    relevance: { type: "string", default: "medium" },
  },
  handler: async (args) => {
    return { saved: true, finding: args };
  },
});

async function research(topic: string) {
  const agent = new Agent("anthropic/claude-sonnet-4", {
    tools: [saveFinding],
    instructions: `You are a research assistant. Search, extract, analyze, and report.`,
    maxIterations: 50,
  });

  const result = await agent.run(
    `Literature survey on: ${topic}. Write report to research_report.md.`
  );
  console.log(`Cost: $${result.cost.totalCost.toFixed(4)}`);
}

research("retrieval augmented generation techniques 2025-2026");
```

**Why EdgeCrab wins:** Built-in `web_search` + `web_extract` + `web_crawl` means the agent can autonomously find and read web pages. Claude SDK has no web tools. Google ADK has only `google_search` (no extraction). Pydantic AI has zero built-in tools.

---

## Example 4: Customer Support Triage Agent

**Problem:** Incoming support tickets need to be classified, searched against the knowledge base, and either auto-resolved or routed to the right team — 24/7, across Telegram, Slack, and email.

**EdgeCrab capabilities used:** Gateway (multi-platform delivery), `session_search` (find similar past tickets), memory (remember resolution patterns), `delegate_task` (escalate to specialist agents).

### Python

```python
from edgecrab import Agent, Tool

@Tool("search_knowledge_base", description="Search support KB for solutions")
async def search_knowledge_base(query: str, limit: int = 5) -> dict:
    # In production, connect to your actual KB (Zendesk, Notion, Confluence)
    # Here we use EdgeCrab's session search as a demo
    return {
        "results": [
            {"title": "Password Reset Guide", "match": 0.92,
             "solution": "Direct user to /reset-password endpoint"},
            {"title": "API Rate Limits", "match": 0.87,
             "solution": "Default: 100/min. Enterprise: 1000/min. Check plan."},
        ],
        "total": 2,
    }

@Tool("create_ticket", description="Create a support ticket in the tracking system")
async def create_ticket(
    title: str,
    description: str,
    priority: str = "medium",
    assigned_team: str = "general",
) -> dict:
    import uuid
    ticket_id = f"TKT-{uuid.uuid4().hex[:6].upper()}"
    return {
        "ticket_id": ticket_id,
        "status": "created",
        "priority": priority,
        "team": assigned_team,
    }

@Tool("escalate_to_human", description="Escalate to a human agent with context")
async def escalate_to_human(
    reason: str,
    summary: str,
    urgency: str = "normal",
) -> dict:
    return {"escalated": True, "reason": reason, "queue_position": 3}


async def support_agent():
    agent = Agent(
        model="anthropic/claude-sonnet-4",
        tools=[search_knowledge_base, create_ticket, escalate_to_human],
        instructions="""You are a Tier 1 support agent for AcmeCorp.

TRIAGE RULES:
- Password/login issues → search KB, provide self-service link
- Billing questions → create ticket for billing team (priority: high)
- Bug reports → create ticket for engineering (collect repro steps first)
- Feature requests → create ticket for product (priority: low)
- Angry customer → acknowledge, create high-priority ticket, escalate

ALWAYS:
1. Greet the customer warmly
2. Understand the problem before acting
3. Search the KB before creating tickets
4. If KB has a solution, provide it and ask if it helped
5. If not resolved, create a ticket and give the ticket ID
6. Never say "I don't know" — escalate instead""",
        max_iterations=20,
    )

    # Simulate multi-platform support
    conversations = [
        "I can't log in! I've tried resetting my password 3 times!",
        "Your API is returning 429 errors. We're on the enterprise plan.",
        "I want to cancel my subscription. Your product is terrible.",
    ]

    for msg in conversations:
        print(f"\n{'='*60}")
        print(f"Customer: {msg}")
        await agent.new_session()  # Fresh session per ticket
        response = await agent.chat(msg)
        print(f"Agent: {response}")
```

**Why EdgeCrab wins:** Deploy this exact agent to Telegram, Slack, Discord, WhatsApp, and email simultaneously using the gateway. No code changes. No competitor SDK can deliver across 17 platforms.

---

## Example 5: Data Pipeline Agent

**Problem:** You have CSV files that need cleaning, transformation, analysis, and visualization. You want to describe what you need in plain English, and the agent writes and executes the code.

**EdgeCrab capabilities used:** `file_read`, `file_write`, `execute_code` (sandboxed Python execution), `terminal` (run scripts), `vision` (analyze generated charts).

### Python

```python
from edgecrab import Agent

async def data_pipeline(data_file: str, analysis_request: str):
    agent = Agent(
        model="anthropic/claude-sonnet-4",
        instructions="""You are a data engineering agent.

When given a data file and analysis request:
1. Read the file using file_read to understand its structure
2. Write Python scripts using file_write for each transformation step
3. Execute each script using execute_code or terminal
4. If visualization is needed, generate charts and save as PNG
5. Write a summary report with key findings

RULES:
- Always validate data before transformations
- Handle missing values explicitly (drop, fill, or flag)
- Use pandas for tabular data, matplotlib/seaborn for charts
- Save intermediate results so the user can inspect each step
- Include the code in your explanation so the user can reproduce""",
        max_iterations=40,
    )

    result = await agent.run(
        f"Analyze the data in {data_file}. {analysis_request}"
    )
    print(f"Pipeline complete. Cost: ${result.cost.total_cost:.4f}")

import asyncio
asyncio.run(data_pipeline(
    "sales_data.csv",
    "Find the top 10 products by revenue, show monthly trends, "
    "identify any seasonal patterns, and generate a PDF report."
))
```

**Why EdgeCrab wins:** Built-in `execute_code` sandbox + `file_read/write` + `vision` (can analyze generated charts) creates a full data pipeline agent. OpenAI has a hosted code interpreter but you can't customize the environment. Google ADK's code execution is limited. Pydantic AI has nothing.

---

## Example 6: Smart Home Automation Agent

**Problem:** You want an AI that understands your home, responds to voice commands, and orchestrates complex automations across devices — integrated with Home Assistant.

**EdgeCrab capabilities used:** `ha_get_states`, `ha_call_service`, `ha_trigger_automation`, `ha_get_history`, `cron` (scheduled automations), `tts` (voice feedback), `transcribe` (voice input).

### Python

```python
from edgecrab import Agent, Tool

@Tool("get_room_context", description="Get combined sensor data for a room")
async def get_room_context(room: str) -> dict:
    # In production, aggregate from Home Assistant states
    return {
        "room": room,
        "temperature": 22.5,
        "humidity": 45,
        "occupancy": True,
        "lights_on": ["ceiling", "desk_lamp"],
        "windows": "closed",
        "last_motion": "2 minutes ago",
    }

async def smart_home():
    agent = Agent(
        model="anthropic/claude-sonnet-4",
        tools=[get_room_context],
        # Built-in HA tools are available: ha_get_states, ha_call_service, etc.
        instructions="""You are a smart home AI assistant.

You have access to Home Assistant tools for controlling the house.
When the user gives a command:
1. Understand the intent (lighting, climate, security, entertainment)
2. Check current state before making changes
3. Execute the appropriate service calls
4. Confirm what you did and the new state

SAFETY RULES:
- Never unlock doors or disable security without explicit confirmation
- For heating/cooling changes >3°C, confirm with the user first
- Log all security-related actions

SMART BEHAVIORS:
- "Good night" → turn off all lights, lock doors, set thermostat to 19°C
- "Movie mode" → dim living room to 20%, close blinds, turn on TV
- "I'm leaving" → check all windows, turn off lights, arm security""",
        max_iterations=15,
    )

    commands = [
        "Set the living room to movie mode",
        "What's the temperature in the bedroom?",
        "Good night — make sure everything is locked up",
    ]

    for cmd in commands:
        print(f"\nYou: {cmd}")
        response = await agent.chat(cmd)
        print(f"Home AI: {response}")
```

**Why EdgeCrab wins:** No other SDK has native Home Assistant integration (4 built-in tools). Combined with TTS/transcribe tools, you get a full voice-controlled smart home agent. The gateway delivers to any messaging platform.

---

## Example 7: Document Q&A Agent with Memory

**Problem:** Users upload documents and ask questions. The agent remembers context across sessions — "remember last week when we discussed the compliance section?"

**EdgeCrab capabilities used:** `file_read`, `vision` (for PDFs/images), `memory` (persistent knowledge), `session_search` (find past conversations about documents), session persistence.

### Python

```python
from edgecrab import Agent, Tool
import hashlib

@Tool("index_document", description="Index a document for Q&A")
async def index_document(file_path: str) -> dict:
    # Read file content (EdgeCrab's file_read handles PDFs, images, etc.)
    # In production, use a vector database for chunked embeddings
    import os
    stat = os.stat(file_path)
    doc_hash = hashlib.md5(file_path.encode()).hexdigest()[:8]
    return {
        "indexed": True,
        "doc_id": doc_hash,
        "file": file_path,
        "size_kb": stat.st_size // 1024,
    }

@Tool("recall_context", description="Search past conversations about this document")
async def recall_context(query: str) -> dict:
    # This wraps EdgeCrab's session_search
    return {
        "past_discussions": [
            {"date": "2026-04-10", "topic": "compliance section review",
             "key_point": "Section 4.2 needs updating for new regulation"},
        ]
    }


async def document_qa():
    agent = Agent(
        model="anthropic/claude-sonnet-4",
        tools=[index_document, recall_context],
        session_id="doc-qa-project-alpha",  # Persistent across sessions
        instructions="""You are a document Q&A assistant with memory.

CAPABILITIES:
- Read any document (PDF, MD, DOCX, images) using built-in file_read and vision tools
- Remember past discussions using persistent session and memory
- Search across all past conversations using session_search

BEHAVIOR:
- When a user mentions a document, check if it's been discussed before
- Quote specific sections when answering
- If uncertain, say so and suggest where to look
- Remember user preferences (e.g., "prefers bullet points over paragraphs")

MEMORY MANAGEMENT:
- After each significant discussion, save key findings to memory
- When asked "what did we discuss about X?", search both memory and session history""",
        max_iterations=25,
    )

    # Multi-turn conversation
    print(await agent.chat("Index the file contract_draft_v3.pdf"))
    print(await agent.chat("What are the key terms in section 3?"))
    print(await agent.chat("Compare this with what we discussed last week about compliance"))
    print(await agent.chat("Summarize all our findings as a bullet list"))
```

**Why EdgeCrab wins:** SQLite FTS5 session search + persistent memory + vision (for PDF/image analysis) = a document Q&A agent that genuinely remembers across sessions. Claude SDK has no persistence. OpenAI requires external Redis setup.

---

## Example 8: Multi-Agent Development Team

**Problem:** You want a team of specialized agents — one for architecture decisions, one for implementation, one for testing — collaborating on a feature.

**EdgeCrab capabilities used:** `delegate_task` (sub-agent delegation), `fork` (branch conversations), tools (all built-in), cost tracking (per-agent budget).

### Python

```python
from edgecrab import Agent

async def dev_team(feature_request: str):
    # Lead architect agent — delegates to specialists
    architect = Agent(
        model="anthropic/claude-opus-4",  # Best model for architecture
        instructions="""You are a software architect leading a development team.

Your team has specialized agents you can delegate to:
- Use delegate_task for implementation work
- Use delegate_task for writing tests
- You focus on architecture decisions and code review

WORKFLOW:
1. Analyze the feature request
2. Design the architecture (files, interfaces, data flow)
3. Delegate implementation to a sub-agent with specific instructions
4. Delegate test writing to another sub-agent
5. Review the results and iterate if needed
6. Produce a final summary of what was built""",
        max_iterations=50,
    )

    result = await architect.run(
        f"Implement this feature: {feature_request}\n\n"
        "Design the architecture first, then delegate implementation "
        "and testing to sub-agents. Review their work."
    )

    print(f"\n{'='*60}")
    print(f"Feature complete!")
    print(f"Total cost: ${result.cost.total_cost:.4f}")
    print(f"API calls: {result.api_calls}")
    return result

import asyncio
asyncio.run(dev_team(
    "Add a /health endpoint to the API that returns server status, "
    "uptime, database connection status, and version info. "
    "Include unit tests and integration tests."
))
```

### Rust

```rust
use edgecrab_sdk::prelude::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let architect = Agent::new("anthropic/claude-opus-4")
        .max_iterations(50)
        .build()?;

    let result = architect.run(
        "Design and implement a REST API for user management. \
         Delegate implementation and test writing to sub-agents."
    ).await?;

    println!("Total cost: ${:.4}", result.cost.total_cost);
    println!("API calls: {}", result.api_calls);
    Ok(())
}
```

**Why EdgeCrab wins:** Built-in `delegate_task` creates sub-agents that inherit the full tool set but run independently. The architect can delegate, review, and iterate — all tracked with per-agent cost. OpenAI has "handoffs" but they're one-way transfers, not supervised delegation.

---

## Example 9: Competitive Intelligence Monitor

**Problem:** You want daily monitoring of competitor websites, pricing changes, and product launches — with structured reports delivered to Slack.

**EdgeCrab capabilities used:** `web_search`, `web_extract`, `web_crawl`, `file_write`, `memory` (remember past competitor data), `cron` (daily schedule), `send_message` (deliver to Slack).

### Python

```python
from edgecrab import Agent, Tool
import json
from datetime import datetime

@Tool("compare_with_history", description="Compare current competitor data with stored history")
async def compare_with_history(competitor: str, current_data: dict) -> dict:
    # In production, load from database. Here, demonstrate the concept.
    # Memory tool stores previous snapshots.
    return {
        "competitor": competitor,
        "changes_detected": [
            {"field": "pricing", "old": "$49/mo", "new": "$59/mo", "change": "+20%"},
        ],
        "new_features": ["AI assistant", "API v2"],
        "removed_features": [],
    }

@Tool("generate_intel_report", description="Generate a structured competitive intelligence report")
async def generate_intel_report(
    competitors: list,
    findings: list,
    date: str = None,
) -> dict:
    report_date = date or datetime.now().strftime("%Y-%m-%d")
    report = {
        "date": report_date,
        "competitors_analyzed": len(competitors),
        "total_changes": len(findings),
        "summary": f"Analyzed {len(competitors)} competitors. Found {len(findings)} changes.",
        "findings": findings,
    }
    return report


async def competitive_intel():
    agent = Agent(
        model="anthropic/claude-sonnet-4",
        tools=[compare_with_history, generate_intel_report],
        session_id="competitive-intel",
        instructions="""You are a competitive intelligence analyst.

DAILY ROUTINE:
1. For each competitor, search for their latest updates:
   - Pricing page changes
   - New product announcements
   - Blog posts / press releases
   - Job postings (indicates focus areas)
2. Extract key information using web_extract
3. Compare with historical data using compare_with_history
4. Save current state to memory for next comparison
5. Generate a structured report
6. If significant changes detected, flag as HIGH PRIORITY

COMPETITORS TO MONITOR:
- CompetitorA (https://competitora.com)
- CompetitorB (https://competitorb.com)
- CompetitorC (https://competitorc.com)

OUTPUT FORMAT:
- Changes: numbered list with [HIGH/MED/LOW] severity
- Market impact: one paragraph analysis
- Recommended actions: bullet points""",
        max_iterations=40,
    )

    result = await agent.run(
        "Run today's competitive intelligence sweep. "
        "Compare with last week's data and highlight significant changes."
    )
    print(f"Intel report complete. Cost: ${result.cost.total_cost:.4f}")
```

**Why EdgeCrab wins:** `web_search` + `web_extract` + `web_crawl` + persistent memory + multi-platform delivery = a full CI pipeline. No competitor SDK has all five built in.

---

## Example 10: Personal Coding Assistant (CLI Agent)

**Problem:** You want a coding assistant that lives in your terminal, understands your entire codebase, remembers your preferences, and can write, test, and debug code autonomously.

**EdgeCrab capabilities used:** ALL built-in tools — this is EdgeCrab's flagship use case. `file_read`, `file_write`, `file_search`, `file_patch`, `terminal`, `execute_code`, `memory`, `session_search`, `web_search`, `delegate_task`, LSP tools (25+).

### Python

```python
from edgecrab import Agent

async def coding_assistant():
    agent = Agent(
        model="anthropic/claude-sonnet-4",
        session_id="my-project",  # Remember project context across sessions
        instructions="""You are a senior software engineer working on this project.

You have full access to the codebase and can:
- Read and write files
- Search code with ripgrep
- Run terminal commands
- Execute code in a sandbox
- Search the web for documentation
- Use LSP for go-to-definition, find-references, hover info
- Remember project decisions across sessions

WORKFLOW:
1. Understand the request fully before writing code
2. Read relevant existing code first
3. Make minimal, targeted changes
4. Write tests for new code
5. Run tests to verify
6. Explain what you changed and why

PREFERENCES:
- Check memory for project-specific preferences first
- Write idiomatic code for the project's language
- Keep PRs small and focused
- Always run the linter after changes""",
        max_iterations=90,  # Full autonomy
    )

    # Interactive session
    while True:
        user_input = input("\nYou: ").strip()
        if user_input.lower() in ("quit", "exit", "q"):
            break
        if not user_input:
            continue

        result = await agent.run(user_input)
        print(f"\nAssistant: {result.response}")
        print(f"  (cost: ${result.cost.total_cost:.4f}, "
              f"tools: {result.api_calls} calls)")

import asyncio
asyncio.run(coding_assistant())
```

**Why EdgeCrab wins:** This IS EdgeCrab. The full CLI experience — 100+ tools, LSP integration, session persistence, memory, delegation — all available programmatically. No other SDK can offer this because no other SDK IS a complete agent runtime.

---

## Summary: What Each Example Proves

```
+------------------------------------------------------------------------+
|  Example                      | Key EdgeCrab Advantage                 |
+-------------------------------+----------------------------------------+
|  1. Code Reviewer             | Built-in file + terminal + web tools   |
|  2. Infra Monitor             | Sessions + gateway + cron              |
|  3. Research Assistant         | web_search + web_extract + web_crawl   |
|  4. Support Triage            | Gateway: 17 platforms, zero code change|
|  5. Data Pipeline             | execute_code + file I/O + vision       |
|  6. Smart Home                | Native Home Assistant tools (4)        |
|  7. Document Q&A              | Session search + memory + vision       |
|  8. Multi-Agent Team          | delegate_task + fork + cost tracking   |
|  9. Competitive Intel         | Web tools + memory + cron + messaging  |
|  10. Coding Assistant         | ALL 100+ tools — this IS EdgeCrab      |
|  11. Browser Chat Agent       | WASM SDK — runs entirely in browser    |
+------------------------------------------------------------------------+
```

---

## Example 11: Browser Chat Agent (WASM SDK)

**Scenario:** An AI-powered documentation assistant running entirely in the browser — no backend server needed. The EdgeCrab WASM SDK compiles the Rust agent core to WebAssembly, enabling the ReAct loop, streaming, and custom tools to execute client-side.

**Why EdgeCrab:** No other agent framework can run natively in the browser. Claude SDK, OpenAI Agents, Google ADK — all require a server. EdgeCrab's WASM SDK brings the full agent loop (minus filesystem/terminal tools) to any web page.

```typescript
// browser-chat-agent/src/main.ts
import init, { Agent, Tool } from "@edgecrab/wasm";
import { IndexedDBSessionStore } from "@edgecrab/wasm/adapters";

// Initialize the WASM module once
await init();

// ── Custom tools using browser APIs ─────────────────────────
const searchDocs = Tool.create({
  name: "search_docs",
  description: "Search the product documentation index",
  parameters: {
    query: { type: "string", description: "Search query" },
    section: { type: "string", description: "Section filter (optional)" },
  },
  handler: async (args: { query: string; section?: string }) => {
    // Uses browser fetch — CORS-safe because same-origin
    const url = `/api/docs/search?q=${encodeURIComponent(args.query)}` +
      (args.section ? `&section=${encodeURIComponent(args.section)}` : "");
    const res = await fetch(url);
    const results = await res.json();
    return JSON.stringify(results);
  },
});

const getUserContext = Tool.create({
  name: "get_user_context",
  description: "Get the current user's profile and preferences",
  parameters: {},
  handler: async () => {
    // Read from browser localStorage
    const profile = JSON.parse(localStorage.getItem("user_profile") ?? "{}");
    const prefs = JSON.parse(localStorage.getItem("user_prefs") ?? "{}");
    return JSON.stringify({ profile, prefs });
  },
});

const copyToClipboard = Tool.create({
  name: "copy_to_clipboard",
  description: "Copy text to the user's clipboard",
  parameters: {
    text: { type: "string", description: "Text to copy" },
  },
  handler: async (args: { text: string }) => {
    await navigator.clipboard.writeText(args.text);
    return JSON.stringify({ copied: true, length: args.text.length });
  },
});

// ── Agent setup ─────────────────────────────────────────────
const agent = new Agent("openai/gpt-4o", {
  apiKey: localStorage.getItem("openai_key") ?? "",
  tools: [searchDocs, getUserContext, copyToClipboard],
  instructions: `You are a documentation assistant for our product.
    Use search_docs to find relevant articles.
    Use get_user_context to personalize answers.
    When sharing code snippets, offer to copy them to clipboard.`,
  maxIterations: 15,
  sessionStore: new IndexedDBSessionStore("docs-assistant"),
  sessionId: `session-${Date.now()}`,
});

// ── Streaming into DOM ──────────────────────────────────────
const chatEl = document.getElementById("chat")!;
const inputEl = document.getElementById("input") as HTMLInputElement;

async function sendMessage(userMessage: string) {
  // Render user message
  chatEl.innerHTML += `<div class="user-msg">${escapeHtml(userMessage)}</div>`;

  // Stream agent response
  const responseEl = document.createElement("div");
  responseEl.className = "agent-msg";
  chatEl.appendChild(responseEl);

  for await (const event of agent.stream(userMessage)) {
    switch (event.type) {
      case "token":
        responseEl.textContent += event.text;
        break;
      case "tool_exec":
        responseEl.innerHTML += `<span class="tool-call">🔧 ${event.name}</span>`;
        break;
      case "done":
        responseEl.innerHTML += `<span class="cost">$${event.cost.toFixed(4)}</span>`;
        break;
    }
  }

  chatEl.scrollTop = chatEl.scrollHeight;
}
```

**Capability Mapping:**
| What it does | EdgeCrab feature used |
|-------------|---------------------|
| Agent loop in browser | WASM SDK (`@edgecrab/wasm`) |
| Token streaming to DOM | `agent.stream()` → AsyncIterable |
| Custom browser tools | `Tool.create()` with JS `fetch`, `localStorage`, `clipboard` |
| Session persistence | IndexedDB adapter (conversations survive page reloads) |
| Cost tracking | Built-in per-conversation cost in `done` event |
| No server needed | Entire agent runs client-side in WASM |

**Deployment:**
```bash
# Build and bundle
npm install @edgecrab/wasm
npx vite build  # bundles .wasm + glue into dist/

# Deploy anywhere static files are served:
# - Cloudflare Pages, Vercel, Netlify, GitHub Pages
# - No server, no Lambda, no container
```

---

## Brutal Honest Assessment

### What's Strong
- These are real use cases people actually pay for, not toy demos
- Each example is complete and runnable (with the SDK that needs to be built)
- The capability mapping shows genuine differentiation vs competitors
- Multi-language examples (Python, Node.js, Rust) prove tri-language parity
- **Example 11 (WASM Browser Agent) is a unique differentiator** — no competitor can do this

### What's Weak
- **These all depend on an SDK that doesn't exist yet.** The `@Tool` decorator, `Agent.run()` returning `ConversationResult`, etc. — all proposed API that needs to be built
- **The Node.js and Rust examples are thinner.** Most real-world agent users will be Python developers. The Node.js and Rust examples should be expanded post-launch based on actual user patterns.
- **No benchmarks.** How long does Example 1 (code review) actually take? What does it cost? Without real performance data, the claims are aspirational.
- **Custom tool examples are mostly thin wrappers.** The real value is in the *built-in* tools, not the custom ones. This is actually an advantage (less boilerplate) but users may undervalue the SDK if they think "I could just call subprocess myself."
- **No error handling shown.** Production agents need retry logic, fallback models, graceful degradation. None of the examples show this. See `04-TUTORIAL.md` Part 10 for error handling patterns.
- **WASM example (11) requires API key on client-side.** In production, you'd proxy through a backend to protect the key. This isn't shown.

### What Competitors Can Match
- **Examples 1, 5, 10:** Claude SDK can do coding tasks well (it IS Claude Code)
- **Example 3:** OpenAI Agents SDK has hosted web search
- **Example 8:** Google ADK has `sub_agents` for multi-agent patterns
- **Example 7:** Pydantic AI has structured output validation (better type safety on responses)

### What ONLY EdgeCrab Can Do
- **Example 2, 4, 9:** Multi-platform delivery (17 platforms) — nobody else has this
- **Example 6:** Native Home Assistant integration — nobody else has this
- **Example 10:** Full 100+ tool CLI agent as a library — nobody else has this
- **Example 11:** Browser-native agent via WASM — nobody else can compile their agent to run client-side
- **Cross-session memory + FTS5 search** — built into every example, zero setup
