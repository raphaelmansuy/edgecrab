/**
 * E2E-style tests for the EdgeCrab Node.js native SDK.
 *
 * These tests validate the API surface, type exports, and wrapper logic.
 * Tests that require a real LLM API key are skipped unless EDGECRAB_E2E=1.
 */

import { describe, it, expect } from "vitest";
import { Tool, Message, Role } from "../wrapper.js";
import { Agent, Config, edgecrabHome, ensureEdgecrabHome } from "../index.js";

// ── Import surface tests ─────────────────────────────────────────────

describe("SDK exports", () => {
  it("Tool class is importable", () => {
    expect(Tool).toBeDefined();
    expect(typeof Tool.create).toBe("function");
  });

  it("Agent and Config classes are importable", () => {
    expect(Agent).toBeDefined();
    expect(Config).toBeDefined();
    expect(typeof Config.loadFrom).toBe("function");
    expect(typeof Config.loadProfile).toBe("function");
  });

  it("home helpers are importable", () => {
    expect(typeof edgecrabHome).toBe("function");
    expect(typeof ensureEdgecrabHome).toBe("function");

    const home = edgecrabHome();
    expect(typeof home).toBe("string");
    expect(home.length).toBeGreaterThan(0);
    expect(ensureEdgecrabHome()).toBe(home);
  });

  it("Message class is importable", () => {
    expect(Message).toBeDefined();
    expect(typeof Message.system).toBe("function");
    expect(typeof Message.user).toBe("function");
    expect(typeof Message.assistant).toBe("function");
  });

  it("Role enum is importable", () => {
    expect(Role).toBeDefined();
    expect(Role.System).toBe("system");
    expect(Role.User).toBe("user");
    expect(Role.Assistant).toBe("assistant");
    expect(Role.Tool).toBe("tool");
  });
});

// ── Tool tests ───────────────────────────────────────────────────────

describe("Tool.create()", () => {
  it("creates a tool with required fields", () => {
    const tool = Tool.create({
      name: "test_tool",
      description: "A test tool",
      parameters: {
        type: "object",
        properties: {
          input: { type: "string" },
        },
        required: ["input"],
      },
      handler: async (args) => JSON.stringify({ result: args.input }),
    });

    expect(tool).toBeDefined();
    expect(tool.name).toBe("test_tool");
    expect(tool.description).toBe("A test tool");
    expect(tool.parameters).toEqual({
      type: "object",
      properties: {
        input: { type: "string" },
      },
      required: ["input"],
    });
    expect(typeof tool.handler).toBe("function");
  });

  it("generates OpenAI-compatible schema", () => {
    const tool = Tool.create({
      name: "fetch_url",
      description: "Fetch a URL",
      parameters: {
        type: "object",
        properties: {
          url: { type: "string", description: "URL to fetch" },
        },
        required: ["url"],
      },
      handler: async () => "ok",
    });

    const schema = tool.toFunctionSchema();
    expect(schema).toEqual({
      type: "function",
      function: {
        name: "fetch_url",
        description: "Fetch a URL",
        parameters: {
          type: "object",
          properties: {
            url: { type: "string", description: "URL to fetch" },
          },
          required: ["url"],
        },
      },
    });
  });
});

// ── Message tests ────────────────────────────────────────────────────

describe("Message", () => {
  it("creates system messages", () => {
    const msg = Message.system("You are a helpful assistant.");
    expect(msg.role).toBe("system");
    expect(msg.content).toBe("You are a helpful assistant.");
  });

  it("creates user messages", () => {
    const msg = Message.user("Hello!");
    expect(msg.role).toBe("user");
    expect(msg.content).toBe("Hello!");
  });

  it("creates assistant messages", () => {
    const msg = Message.assistant("Hi there!");
    expect(msg.role).toBe("assistant");
    expect(msg.content).toBe("Hi there!");
  });

  it("creates tool messages", () => {
    const msg = Message.tool("call_123", '{"result": "ok"}');
    expect(msg.role).toBe("tool");
    expect(msg.content).toBe('{"result": "ok"}');
    expect(msg.tool_call_id).toBe("call_123");
  });
});

// ── E2E tests (require EDGECRAB_E2E=1 and valid API keys) ───────────

const E2E = process.env.EDGECRAB_E2E === "1";

describe.skipIf(!E2E)("E2E: Agent", () => {
  it("simple chat", async () => {
    const { Agent } = await import("../index.js");
    const agent = new Agent({ model: "copilot/gpt-5-mini" });
    const reply = await agent.chat("Say 'hello' and nothing else.");
    expect(reply.toLowerCase()).toContain("hello");
  });

  it("stream returns events", async () => {
    const { Agent } = await import("../index.js");
    const agent = new Agent({ model: "copilot/gpt-5-mini" });
    const events = await agent.stream("Say 'hello' and nothing else.");
    expect(events.length).toBeGreaterThan(0);
    const tokens = events
      .filter((e) => e.eventType === "token")
      .map((e) => e.data)
      .join("");
    expect(tokens.toLowerCase()).toContain("hello");
  });

  it("memory read/write/remove", async () => {
    const { Agent } = await import("../index.js");
    const agent = new Agent({ model: "copilot/gpt-5-mini" });
    const mem = agent.memory;

    await mem.write("memory", "test-entry-12345");
    const content = await mem.read("memory");
    expect(content).toContain("test-entry-12345");

    const removed = await mem.remove("memory", "test-entry-12345");
    expect(removed).toBe(true);
  });

  it("conversation result has cost", async () => {
    const { Agent } = await import("../index.js");
    const agent = new Agent({ model: "copilot/gpt-5-mini" });
    const result = await agent.run("Say 'hello' and nothing else.");
    expect(result.response.toLowerCase()).toContain("hello");
    expect(result.apiCalls).toBeGreaterThan(0);
    expect(typeof result.inputTokens).toBe("number");
    expect(typeof result.outputTokens).toBe("number");
    expect(typeof result.totalCost).toBe("number");
  });
});
