import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { EdgeCrabClient, EdgeCrabError } from '../src/client.js';

const MOCK_COMPLETION = {
  id: 'chatcmpl-test123',
  object: 'chat.completion',
  created: 1700000000,
  model: 'anthropic/claude-sonnet-4-20250514',
  choices: [
    {
      index: 0,
      message: { role: 'assistant', content: "Hello! I'm EdgeCrab." },
      finish_reason: 'stop',
    },
  ],
  usage: { prompt_tokens: 10, completion_tokens: 8, total_tokens: 18 },
};

const MOCK_MODELS = {
  data: [
    { id: 'anthropic/claude-sonnet-4-20250514', object: 'model', owned_by: 'anthropic' },
    { id: 'openai/gpt-4', object: 'model', owned_by: 'openai' },
  ],
};

const MOCK_HEALTH = { status: 'ok', version: '0.1.0' };

// Mock global fetch
const mockFetch = vi.fn();
vi.stubGlobal('fetch', mockFetch);

function jsonResponse(data: unknown, status = 200) {
  return new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });
}

function errorResponse(data: unknown, status: number) {
  return new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });
}

describe('EdgeCrabClient', () => {
  beforeEach(() => {
    mockFetch.mockReset();
  });

  describe('chat()', () => {
    it('returns the assistant reply', async () => {
      mockFetch.mockResolvedValueOnce(jsonResponse(MOCK_COMPLETION));
      const client = new EdgeCrabClient();
      const reply = await client.chat('Hello');
      expect(reply).toBe("Hello! I'm EdgeCrab.");
    });

    it('sends system prompt when provided', async () => {
      mockFetch.mockResolvedValueOnce(jsonResponse(MOCK_COMPLETION));
      const client = new EdgeCrabClient();
      await client.chat('Hello', { system: 'Be concise' });

      const [, init] = mockFetch.mock.calls[0];
      const body = JSON.parse(init.body);
      expect(body.messages[0].role).toBe('system');
      expect(body.messages[1].role).toBe('user');
    });
  });

  describe('createCompletion()', () => {
    it('returns structured response', async () => {
      mockFetch.mockResolvedValueOnce(jsonResponse(MOCK_COMPLETION));
      const client = new EdgeCrabClient();
      const resp = await client.createCompletion([{ role: 'user', content: 'Hi' }]);
      expect(resp.id).toBe('chatcmpl-test123');
      expect(resp.choices).toHaveLength(1);
      expect(resp.choices[0].message.content).toBe("Hello! I'm EdgeCrab.");
      expect(resp.usage?.total_tokens).toBe(18);
    });
  });

  describe('listModels()', () => {
    it('returns model list', async () => {
      mockFetch.mockResolvedValueOnce(jsonResponse(MOCK_MODELS));
      const client = new EdgeCrabClient();
      const models = await client.listModels();
      expect(models).toHaveLength(2);
      expect(models[0].id).toBe('anthropic/claude-sonnet-4-20250514');
    });
  });

  describe('health()', () => {
    it('returns health status', async () => {
      mockFetch.mockResolvedValueOnce(jsonResponse(MOCK_HEALTH));
      const client = new EdgeCrabClient();
      const h = await client.health();
      expect(h.status).toBe('ok');
      expect(h.version).toBe('0.1.0');
    });
  });

  describe('error handling', () => {
    it('throws EdgeCrabError on 401', async () => {
      mockFetch.mockResolvedValueOnce(
        errorResponse({ error: { message: 'Unauthorized' } }, 401),
      );
      const client = new EdgeCrabClient();
      await expect(client.chat('Hello')).rejects.toThrow(EdgeCrabError);
      try {
        await client.chat('Hello');
      } catch (e) {
        // Already tested above
      }
    });

    it('throws with status code', async () => {
      mockFetch.mockResolvedValueOnce(
        errorResponse({ error: { message: 'Forbidden' } }, 403),
      );
      const client = new EdgeCrabClient();
      try {
        await client.chat('Hello');
      } catch (e: unknown) {
        expect(e).toBeInstanceOf(EdgeCrabError);
        expect((e as EdgeCrabError).statusCode).toBe(403);
      }
    });
  });

  describe('client options', () => {
    it('uses custom base URL', async () => {
      mockFetch.mockResolvedValueOnce(jsonResponse(MOCK_HEALTH));
      const client = new EdgeCrabClient({ baseUrl: 'http://custom:9999' });
      await client.health();

      const [url] = mockFetch.mock.calls[0];
      expect(url).toContain('http://custom:9999');
    });

    it('sends API key in Authorization header', async () => {
      mockFetch.mockResolvedValueOnce(jsonResponse(MOCK_HEALTH));
      const client = new EdgeCrabClient({ apiKey: 'test-key-123' });
      await client.health();

      const [, init] = mockFetch.mock.calls[0];
      expect(init.headers['Authorization']).toBe('Bearer test-key-123');
    });
  });
});
