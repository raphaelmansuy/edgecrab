# EdgeCrab LinkedIn Post

---

Most coding agents feel like duct-taping a Python script to an API.
You install a 2 GB venv, wait 10 seconds to boot, and hope the runtime does not interfere with your project.
There is a better way.

👉 WHY EdgeCrab exists

The Nous Hermes Agent project pioneered something important: an agent that reasons autonomously, remembers across sessions, learns new skills, and respects user alignment. Thousands of developers adopted it. But it ran on Python, and Python has a cost: cold starts, bundled interpreters, runtime fragility.

EdgeCrab was built to answer one question: what if you kept the Nous Hermes soul — the reasoning loop, the memory, the skills, the plugins, the alignment — and rebuilt the entire engine in Rust?

The answer is a single ~49 MB binary. No Python. No Node. No runtime. Just run it.

👉 Full drop-in compatibility with Nous Hermes

If you already use Nous Hermes Agent, EdgeCrab is a zero-rework upgrade:

Every skill you wrote for Hermes (.md files in ~/.hermes/skills/) works in EdgeCrab unchanged.
Every plugin drops in.
Memories migrate with a single command: edgecrab migrate
The 90+ turn ReAct reasoning loop behaves identically.
Compatible toolsets: file, web, terminal, vision, memory, delegation, MCP, and more.

This is not a rewrite that breaks your workflow. It is the same workflow, compiled.

👉 Where EdgeCrab is sharper for coding

For coding specifically, Rust brings properties that Python never could:

No GIL. Parallel tool execution with real OS threads. File reads, web searches, and shell commands run concurrently inside a single agent turn.
Safety-first I/O. Every file operation is path-jailed before execution. SSRF guards block private network fetch. No silent privilege escalation.
LSP integration. EdgeCrab speaks the Language Server Protocol natively. It understands Go to Definition, Find References, and Rename Symbol — not by running your language server as a subprocess child but by talking its protocol directly.
Sandboxed code execution. Run arbitrary code inside a per-session Docker or process jail with resource constraints, not in your live shell.
1 629 tests. The tooling is verified at the unit level before any user touches it.

👉 The value proposition

Nous Hermes: soul, alignment, reasoning. Python.
EdgeCrab: same soul. Same alignment. Rust speed. Native binary. Gateway presence on 15 messaging platforms (Telegram, Discord, Slack, WhatsApp, Signal, Matrix, and more). Zero cold start.

Install in 30 seconds:
npx edgecrab setup
or
pip install edgecrab-cli

Your Hermes skills work on day one.

👉 Try it

https://edgecrab.com
github.com/raphaelmansuy/edgecrab

If you built anything on Nous Hermes Agent — or always wanted to but not the Python overhead — EdgeCrab is for you.

---
Character count target: < 3000
