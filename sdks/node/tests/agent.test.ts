import { describe, it, expect, vi, beforeEach } from 'vitest';
import { Agent } from '../src/agent.js';
import {
  InterruptedError,
  MaxTurnsExceededError,
  AuthenticationError,
  RateLimitError,
  ServerError,
  EdgeCrabError,
} from '../src/client.js';

const MOCK_COMPLETION = {
  id: 'chatcmpl-test123',
  object: 'chat.completion',
  created: 1700000000,
  model: 'anthropic/claude-sonnet-4-20250514',
  choices: [
    {
      index: 0,
      message: { role: 'assistant' as const, content: "Hello! I'm EdgeCrab." },
      finish_reason: 'stop',
    },
  ],
  usage: { prompt_tokens: 10, completion_tokens: 8, total_tokens: 18 },
};

const MOCK_MODELS = {
  data: [
    { id: 'anthropic/claude-sonnet-4-20250514', object: 'model', owned_by: 'anthropic' },
  ],
};

const MOCK_HEALTH = { status: 'ok', version: '0.1.0' };

const mockFetch = vi.fn();
vi.stubGlobal('fetch', mockFetch);

function jsonResponse(data: unknown) {
  return new Response(JSON.stringify(data), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  });
}

describe('Agent', () => {
  beforeEach(() => {
    mockFetch.mockReset();
  });

  describe('constructor', () => {
    it('uses default model', () => {
      const agent = new Agent();
      expect(agent.model).toBe('anthropic/claude-sonnet-4-20250514');
    });

    it('accepts custom model', () => {
      const agent = new Agent({ model: 'openai/gpt-4' });
      expect(agent.model).toBe('openai/gpt-4');
    });

    it('auto-generates session ID', () => {
      const agent = new Agent();
      expect(agent.sessionId).toBeTruthy();
      expect(agent.sessionId.length).toBeGreaterThan(0);
    });

    it('accepts explicit session ID', () => {
      const agent = new Agent({ sessionId: 'my-session' });
      expect(agent.sessionId).toBe('my-session');
    });
  });

  describe('chat()', () => {
    it('returns assistant reply', async () => {
      mockFetch.mockResolvedValueOnce(jsonResponse(MOCK_COMPLETION));
      const agent = new Agent();
      const reply = await agent.chat('Hello');
      expect(reply).toBe("Hello! I'm EdgeCrab.");
    });

    it('maintains conversation history', async () => {
      mockFetch.mockImplementation(() => Promise.resolve(jsonResponse(MOCK_COMPLETION)));
      const agent = new Agent();

      await agent.chat('First');
      expect(agent.getTurnCount()).toBe(1);
      expect(agent.getMessages()).toHaveLength(2); // user + assistant

      await agent.chat('Second');
      expect(agent.getTurnCount()).toBe(2);
      expect(agent.getMessages()).toHaveLength(4);
    });

    it('includes system prompt in history', async () => {
      mockFetch.mockResolvedValueOnce(jsonResponse(MOCK_COMPLETION));
      const agent = new Agent({ systemPrompt: 'Be helpful' });

      await agent.chat('Hello');
      const msgs = agent.getMessages();
      expect(msgs[0].role).toBe('system');
      expect(msgs[0].content).toBe('Be helpful');
      expect(msgs).toHaveLength(3); // system + user + assistant
    });

    it('fires onTurn callback', async () => {
      mockFetch.mockResolvedValueOnce(jsonResponse(MOCK_COMPLETION));
      const onTurn = vi.fn();
      const agent = new Agent({ onTurn });

      await agent.chat('Hello');
      expect(onTurn).toHaveBeenCalledWith(1, expect.objectContaining({ role: 'assistant' }));
    });
  });

  describe('run()', () => {
    it('returns AgentResult', async () => {
      mockFetch.mockResolvedValueOnce(jsonResponse(MOCK_COMPLETION));
      const agent = new Agent();
      const result = await agent.run('Hello');

      expect(result.response).toBe("Hello! I'm EdgeCrab.");
      expect(result.turnsUsed).toBe(1);
      expect(result.finishedNaturally).toBe(true);
      expect(result.interrupted).toBe(false);
      expect(result.maxTurnsExceeded).toBe(false);
      expect(result.sessionId).toBe(agent.sessionId);
      expect(result.model).toBe(agent.model);
      expect(result.messages).toHaveLength(2);
    });

    it('accepts conversationHistory', async () => {
      mockFetch.mockResolvedValueOnce(jsonResponse(MOCK_COMPLETION));
      const agent = new Agent();
      const result = await agent.run('Follow up', {
        conversationHistory: [
          { role: 'user', content: 'Prior message' },
          { role: 'assistant', content: 'Prior reply' },
        ],
      });
      // 2 prior + 1 user + 1 assistant = 4
      expect(result.messages).toHaveLength(4);
      expect(result.messages[0].content).toBe('Prior message');
    });
  });

  describe('reset()', () => {
    it('clears history and generates new session ID', async () => {
      mockFetch.mockResolvedValue(jsonResponse(MOCK_COMPLETION));
      const agent = new Agent();

      await agent.chat('Hello');
      const oldSession = agent.sessionId;

      agent.reset();
      expect(agent.getTurnCount()).toBe(0);
      expect(agent.getMessages()).toHaveLength(0);
      expect(agent.sessionId).not.toBe(oldSession);
    });

    it('preserves system prompt', () => {
      const agent = new Agent({ systemPrompt: 'Be helpful' });
      agent.reset();
      expect(agent.getMessages()).toHaveLength(1);
      expect(agent.getMessages()[0].role).toBe('system');
    });
  });

  describe('addMessage()', () => {
    it('injects a message into history', () => {
      const agent = new Agent();
      agent.addMessage('user', 'injected');
      expect(agent.getMessages()).toHaveLength(1);
      expect(agent.getMessages()[0].content).toBe('injected');
    });
  });

  describe('usage accumulation', () => {
    it('accumulates token usage across turns', async () => {
      mockFetch.mockImplementation(() => Promise.resolve(jsonResponse(MOCK_COMPLETION)));
      const agent = new Agent();

      await agent.chat('First');
      await agent.chat('Second');

      const usage = agent.getUsage();
      expect(usage.total_tokens).toBe(36); // 18 * 2
    });
  });

  describe('listModels()', () => {
    it('returns models from server', async () => {
      mockFetch.mockResolvedValueOnce(jsonResponse(MOCK_MODELS));
      const agent = new Agent();
      const models = await agent.listModels();
      expect(models).toHaveLength(1);
    });
  });

  describe('health()', () => {
    it('returns health status', async () => {
      mockFetch.mockResolvedValueOnce(jsonResponse(MOCK_HEALTH));
      const agent = new Agent();
      const h = await agent.health();
      expect(h.status).toBe('ok');
    });
  });

  describe('interrupt()', () => {
    it('sets and clears interrupt flag', () => {
      const agent = new Agent();
      expect(agent.isInterrupted).toBe(false);
      agent.interrupt();
      expect(agent.isInterrupted).toBe(true);
      agent.clearInterrupt();
      expect(agent.isInterrupted).toBe(false);
    });

    it('chat throws InterruptedError when interrupted', async () => {
      const agent = new Agent();
      agent.interrupt();
      await expect(agent.chat('Hello')).rejects.toThrow(InterruptedError);
    });

    it('run captures interrupt gracefully', async () => {
      const agent = new Agent();
      agent.interrupt();
      const result = await agent.run('Hello');
      expect(result.interrupted).toBe(true);
      expect(result.finishedNaturally).toBe(false);
    });

    it('reset clears interrupt', () => {
      const agent = new Agent();
      agent.interrupt();
      agent.reset();
      expect(agent.isInterrupted).toBe(false);
    });
  });

  describe('max turns enforcement', () => {
    it('throws MaxTurnsExceededError when limit reached', async () => {
      mockFetch.mockImplementation(() => Promise.resolve(jsonResponse(MOCK_COMPLETION)));
      const agent = new Agent({ maxTurns: 1 });
      await agent.chat('First'); // OK
      await expect(agent.chat('Second')).rejects.toThrow(MaxTurnsExceededError);
    });

    it('run captures max turns exceeded', async () => {
      const agent = new Agent({ maxTurns: 0 });
      const result = await agent.run('Hello');
      expect(result.maxTurnsExceeded).toBe(true);
      expect(result.finishedNaturally).toBe(false);
    });
  });

  describe('exportConversation / importConversation', () => {
    it('exports empty state', () => {
      const agent = new Agent();
      const exported = agent.exportConversation();
      expect(exported.messages).toHaveLength(0);
      expect(exported.turnCount).toBe(0);
    });

    it('exports state with history', async () => {
      mockFetch.mockResolvedValueOnce(jsonResponse(MOCK_COMPLETION));
      const agent = new Agent({ systemPrompt: 'Be helpful' });
      await agent.chat('Hello');
      const exported = agent.exportConversation();
      expect(exported.messages).toHaveLength(3); // system + user + assistant
      expect(exported.turnCount).toBe(1);
      expect(exported.usage.total_tokens).toBe(18);
    });

    it('imports and restores state', () => {
      const agent = new Agent();
      agent.importConversation({
        sessionId: 'restored',
        model: 'test',
        messages: [
          { role: 'user', content: 'Hi' },
          { role: 'assistant', content: 'Hey' },
        ],
        turnCount: 1,
        usage: { prompt_tokens: 5, completion_tokens: 3, total_tokens: 8 },
      });
      expect(agent.sessionId).toBe('restored');
      expect(agent.getTurnCount()).toBe(1);
      expect(agent.getMessages()).toHaveLength(2);
      expect(agent.getUsage().total_tokens).toBe(8);
    });

    it('roundtrip preserves state', async () => {
      mockFetch.mockResolvedValueOnce(jsonResponse(MOCK_COMPLETION));
      const agent1 = new Agent();
      await agent1.chat('Hello');
      const exported = agent1.exportConversation();

      const agent2 = new Agent();
      agent2.importConversation(exported);
      expect(agent2.getTurnCount()).toBe(agent1.getTurnCount());
      expect(agent2.getMessages()).toHaveLength(agent1.getMessages().length);
    });
  });

  describe('clone()', () => {
    it('creates independent copy', async () => {
      mockFetch.mockResolvedValueOnce(jsonResponse(MOCK_COMPLETION));
      const agent = new Agent({ systemPrompt: 'Be brief' });
      await agent.chat('Hello');

      const clone = agent.clone();
      expect(clone.getTurnCount()).toBe(agent.getTurnCount());
      expect(clone.getMessages()).toHaveLength(agent.getMessages().length);
      expect(clone.sessionId).not.toBe(agent.sessionId);

      // Independent — modifying clone doesn't affect original
      clone.addMessage('user', 'extra');
      expect(clone.getMessages()).toHaveLength(agent.getMessages().length + 1);
    });
  });

  describe('error hierarchy', () => {
    it('AuthenticationError inherits from EdgeCrabError', () => {
      const err = new AuthenticationError('test', 401);
      expect(err).toBeInstanceOf(EdgeCrabError);
      expect(err.statusCode).toBe(401);
    });

    it('RateLimitError has retryAfter', () => {
      const err = new RateLimitError('test', 30);
      expect(err).toBeInstanceOf(EdgeCrabError);
      expect(err.retryAfter).toBe(30);
    });

    it('ServerError inherits from EdgeCrabError', () => {
      const err = new ServerError('test', 500);
      expect(err).toBeInstanceOf(EdgeCrabError);
    });
  });
});
