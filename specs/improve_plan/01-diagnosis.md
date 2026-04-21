# 01 — Diagnosis: WHY the Agent Fails

## Observed Symptoms (from v0.7.0 screenshot)

```
+-------------------------------------------------------------------+
|  SYMPTOM                  | ROOT CAUSE          | FIX DOCUMENT    |
+-------------------------------------------------------------------+
|  Empty scaffold x2        | content:null allowed | 06-write-file   |
|  terminal without command  | schema not strict    | 04-error-guide  |
|  Suppression cascade      | no corrective hint   | 08-suppression  |
|  python3 -c file read     | terminal escape hatch| 07-terminal     |
|  10m34s command            | no failure escalation| 05-escalation   |
|  wc -c for file size      | 173 tools overwhelm  | 03-tool-reduce  |
+-------------------------------------------------------------------+
```

## Causality Chain

```
    173 tools loaded
         |
         v
    LLM context budget consumed by tool schemas (~30K tokens)
         |
         v
    Less reasoning budget -> more wrong tool calls
         |
         +-------> calls terminal without "command" field
         |              |
         |              v
         |         InvalidArgs error (no schema hint)
         |              |
         |              v
         |         Suppression fires (no corrective guidance)
         |              |
         |              v
         |         LLM falls back to python3 -c (escape hatch)
         |              |
         |              v
         |         10-minute execution with no interruption
         |
         +-------> calls write_file with content:null
                        |
                        v
                   Empty scaffold created (path of least resistance)
                        |
                        v
                   Never patches it (forgot or confused by 173 tools)
                        |
                        v
                   assess_completion sees active todos -> auto-continue
                        |
                        v
                   Budget burned on error spiral
```

## Quantitative Comparison

```
+-----------------------------------------------+
|  Metric            | EdgeCrab  | Hermes Agent  |
+-----------------------------------------------+
|  Core tools        |   173     |    ~36        |
|  Unique schemas    |    77     |    ~30        |
|  LSP tools         |    25     |     0         |
|  Process mgmt      |     7     |     2         |
|  Browser tools     |    15     |    10         |
|  Honcho tools      |     6     |     0*        |
|  MCP tools         |     6     |     2         |
|  content:null      |   yes     |    no         |
|  Error schema hint |    no     |    no**       |
|  Failure escalation|    no     |    no**       |
+-----------------------------------------------+
  * Honcho removed in Hermes — now a memory provider plugin
  ** Neither has this yet — EdgeCrab will lead
```

## Root Causes (ordered by impact)

### RC-1: Tool Explosion (P0)

173 CORE_TOOLS means every API call includes ~77 unique tool schemas.
Each schema averages ~400 tokens. That is ~30,800 tokens of tool
definitions per API call — 24% of a 128K context window consumed
before the conversation even starts.

Hermes Agent ships ~36 core tools. Claude Code ships ~40.
Both maintain high task completion rates.

**WHY 173?** EdgeCrab inherited all Hermes tools, then added:
- 25 LSP tools (fine-grained IDE integration)
- 6 Honcho tools (moved from memory provider)
- 6 MCP tools (protocol-level, not task-level)
- 7 process management tools (granular)
- Extra browser tools (hover, select, wait_for, close)

**FIX**: Move LSP, Honcho, MCP, and extra process tools to
on-demand toolsets. Load only when task context requires them.

### RC-2: content:null Creates Path of Least Resistance (P2)

Hermes Agent's write_file: `content` is `"type": "string"`, required.
EdgeCrab's write_file: `content` is `"type": ["string", "null"]`.

The nullable content creates an attractor state: the LLM can always
call write_file with null to "make progress" without generating content.
The schema description says "prefer writing content directly" but
**schema structure always wins over prose instructions**.

**FIX**: Make content required string. Remove null option.

### RC-3: Errors Lack Self-Correction Data (P1)

When terminal gets `InvalidArgs: missing field "command"`, the error
contains no information about what correct args look like. The LLM
must guess — and with 173 tools to remember, it often guesses wrong.

**FIX**: Include `required_fields` and a `usage_example` in all
InvalidArgs error responses.

### RC-4: Suppression Is Defensive, Not Corrective (P3)

The suppression message says "Repeating identical arguments would be
flaky" — this is library language the LLM doesn't understand. It needs:
1. The original error (what failed)
2. A concrete corrective action
3. Alternative tools to use

**FIX**: Restructure suppression message with original_error + diff.

### RC-5: No Failure Escalation (P2)

assess_completion returns Incomplete when todos remain, causing
auto-continue through error spirals. No mechanism to detect "the
agent is stuck" and escalate to the user.

**FIX**: Track consecutive tool errors. After 3, force NeedsUserInput.

### RC-6: Terminal as Unguarded Escape Hatch (P3)

The terminal description says "don't use cat/head/tail" but the LLM
uses `python3 -c "open(...).read()"` instead (technically not cat).
Anti-patterns need to be detected in the execute() function, not
just in the schema description.

**FIX**: Regex guard in terminal execute() that suggests proper tools.
