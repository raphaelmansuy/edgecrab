/**
 * Low-level HTTP client for the EdgeCrab OpenAI-compatible API.
 */

import type {
  ChatCompletionResponse,
  ChatMessage,
  ClientOptions,
  HealthResponse,
  ModelInfo,
  StreamChunk,
  UsageInfo,
} from './types.js';

const DEFAULT_BASE_URL = 'http://127.0.0.1:8642';
const DEFAULT_TIMEOUT = 120_000; // ms
const DEFAULT_MAX_RETRIES = 3;
const DEFAULT_RETRY_BASE_DELAY = 1000; // ms

// ── Structured Error Hierarchy ──────────────────────────────────

export class EdgeCrabError extends Error {
  statusCode: number | undefined;
  constructor(message: string, statusCode?: number) {
    super(message);
    this.name = 'EdgeCrabError';
    this.statusCode = statusCode;
  }
}

export class AuthenticationError extends EdgeCrabError {
  constructor(message: string, statusCode?: number) {
    super(message, statusCode);
    this.name = 'AuthenticationError';
  }
}

export class RateLimitError extends EdgeCrabError {
  retryAfter: number | undefined;
  constructor(message: string, retryAfter?: number) {
    super(message, 429);
    this.name = 'RateLimitError';
    this.retryAfter = retryAfter;
  }
}

export class ServerError extends EdgeCrabError {
  constructor(message: string, statusCode?: number) {
    super(message, statusCode);
    this.name = 'ServerError';
  }
}

export class TimeoutError extends EdgeCrabError {
  constructor(message = 'Request timed out') {
    super(message);
    this.name = 'TimeoutError';
  }
}

export class ConnectionError extends EdgeCrabError {
  constructor(message = 'Could not connect to EdgeCrab server') {
    super(message);
    this.name = 'ConnectionError';
  }
}

export class MaxTurnsExceededError extends EdgeCrabError {
  maxTurns: number;
  constructor(maxTurns: number) {
    super(`Agent exceeded maximum turns (${maxTurns})`);
    this.name = 'MaxTurnsExceededError';
    this.maxTurns = maxTurns;
  }
}

export class InterruptedError extends EdgeCrabError {
  constructor() {
    super('Agent conversation was interrupted');
    this.name = 'InterruptedError';
  }
}

function classifyError(status: number, detail: string, headers?: Headers): EdgeCrabError {
  const msg = `API error ${status}: ${detail}`;
  if (status === 401 || status === 403) return new AuthenticationError(msg, status);
  if (status === 429) {
    const ra = headers?.get('retry-after');
    return new RateLimitError(msg, ra ? parseFloat(ra) : undefined);
  }
  if (status >= 500) return new ServerError(msg, status);
  return new EdgeCrabError(msg, status);
}

function isRetryable(err: unknown): boolean {
  if (err instanceof ServerError || err instanceof TimeoutError || err instanceof ConnectionError)
    return true;
  if (err instanceof RateLimitError) return true;
  if (err instanceof TypeError && String(err.message).includes('fetch')) return true;
  return false;
}

function buildHeaders(apiKey?: string): Record<string, string> {
  const headers: Record<string, string> = { 'Content-Type': 'application/json' };
  if (apiKey) headers['Authorization'] = `Bearer ${apiKey}`;
  return headers;
}

export class EdgeCrabClient {
  private baseUrl: string;
  private headers: Record<string, string>;
  private timeout: number;
  private maxRetries: number;
  private retryBaseDelay: number;

  constructor(options: ClientOptions = {}) {
    this.baseUrl = (options.baseUrl ?? DEFAULT_BASE_URL).replace(/\/+$/, '');
    this.headers = buildHeaders(options.apiKey);
    this.timeout = options.timeout ?? DEFAULT_TIMEOUT;
    this.maxRetries = options.maxRetries ?? DEFAULT_MAX_RETRIES;
    this.retryBaseDelay = options.retryBaseDelay ?? DEFAULT_RETRY_BASE_DELAY;
  }

  /** Simple chat — send a message, get a reply string. */
  async chat(
    message: string,
    options?: { model?: string; system?: string; temperature?: number; maxTokens?: number },
  ): Promise<string> {
    const messages: ChatMessage[] = [];
    if (options?.system) {
      messages.push({ role: 'system', content: options.system });
    }
    messages.push({ role: 'user', content: message });
    const resp = await this.createCompletion(messages, {
      model: options?.model,
      temperature: options?.temperature,
      maxTokens: options?.maxTokens,
    });
    return resp.choices?.[0]?.message?.content ?? '';
  }

  /** Create a chat completion (non-streaming). */
  async createCompletion(
    messages: ChatMessage[],
    options?: {
      model?: string;
      temperature?: number;
      maxTokens?: number;
      tools?: Record<string, unknown>[];
    },
  ): Promise<ChatCompletionResponse> {
    const body: Record<string, unknown> = {
      model: options?.model ?? 'anthropic/claude-sonnet-4-20250514',
      messages,
      stream: false,
    };
    if (options?.temperature !== undefined) body.temperature = options.temperature;
    if (options?.maxTokens !== undefined) body.max_tokens = options.maxTokens;
    if (options?.tools) body.tools = options.tools;

    return this.postWithRetry(body);
  }

  private async postWithRetry(body: Record<string, unknown>): Promise<ChatCompletionResponse> {
    let lastError: Error | undefined;
    for (let attempt = 0; attempt <= this.maxRetries; attempt++) {
      try {
        const response = await this.fetchJSON<ChatCompletionResponse>('/v1/chat/completions', {
          method: 'POST',
          body: JSON.stringify(body),
        });
        return response;
      } catch (err) {
        lastError = err as Error;
        if (!isRetryable(err) || attempt === this.maxRetries) throw err;
        let delay = this.retryBaseDelay * 2 ** attempt;
        if (err instanceof RateLimitError && err.retryAfter) {
          delay = Math.max(delay, err.retryAfter * 1000);
        }
        await new Promise((r) => setTimeout(r, delay));
      }
    }
    throw lastError!;
  }

  /** Create a streaming chat completion. Returns an async iterator of chunks. */
  async *streamCompletion(
    messages: ChatMessage[],
    options?: {
      model?: string;
      temperature?: number;
      maxTokens?: number;
      tools?: Record<string, unknown>[];
    },
  ): AsyncGenerator<StreamChunk> {
    const body: Record<string, unknown> = {
      model: options?.model ?? 'anthropic/claude-sonnet-4-20250514',
      messages,
      stream: true,
    };
    if (options?.temperature !== undefined) body.temperature = options.temperature;
    if (options?.maxTokens !== undefined) body.max_tokens = options.maxTokens;
    if (options?.tools) body.tools = options.tools;

    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), this.timeout);

    try {
      const response = await fetch(`${this.baseUrl}/v1/chat/completions`, {
        method: 'POST',
        headers: this.headers,
        body: JSON.stringify(body),
        signal: controller.signal,
      });

      if (!response.ok) {
        const text = await response.text();
        throw classifyError(response.status, text, response.headers);
      }

      if (!response.body) {
        throw new EdgeCrabError('No response body for streaming request');
      }

      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let buffer = '';

      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        buffer += decoder.decode(value, { stream: true });
        const lines = buffer.split('\n');
        buffer = lines.pop() ?? '';

        for (const line of lines) {
          const trimmed = line.trim();
          if (!trimmed.startsWith('data: ')) continue;
          const payload = trimmed.slice(6);
          if (payload === '[DONE]') return;
          yield JSON.parse(payload) as StreamChunk;
        }
      }
    } finally {
      clearTimeout(timeoutId);
    }
  }

  /** List available models. */
  async listModels(): Promise<ModelInfo[]> {
    const data = await this.fetchJSON<{ data: ModelInfo[] } | ModelInfo[]>('/v1/models');
    if (Array.isArray(data)) return data;
    return data.data ?? [];
  }

  /** Check server health. */
  async health(): Promise<HealthResponse> {
    return this.fetchJSON<HealthResponse>('/v1/health');
  }

  private async fetchJSON<T>(path: string, init?: RequestInit): Promise<T> {
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), this.timeout);

    try {
      const response = await fetch(`${this.baseUrl}${path}`, {
        ...init,
        headers: { ...this.headers, ...((init?.headers as Record<string, string>) ?? {}) },
        signal: controller.signal,
      });

      if (!response.ok) {
        const text = await response.text();
        let detail = text;
        try {
          const json = JSON.parse(text);
          detail = json?.error?.message ?? text;
        } catch {}
        throw classifyError(response.status, detail, response.headers);
      }

      return (await response.json()) as T;
    } finally {
      clearTimeout(timeoutId);
    }
  }
}
