/**
 * High-level Agent abstraction for the EdgeCrab Node.js SDK.
 *
 * Inspired by hermes-agent's AIAgent — provides a batteries-included Agent
 * object that manages conversation state, streaming, and session lifecycle.
 */

import { randomUUID } from 'node:crypto';
import {
  EdgeCrabClient,
  EdgeCrabError,
  InterruptedError,
  MaxTurnsExceededError,
} from './client.js';
import type {
  AgentOptions,
  AgentResult,
  ChatCompletionResponse,
  ChatMessage,
  ExportedConversation,
  HealthResponse,
  ModelInfo,
  StreamChunk,
  UsageInfo,
} from './types.js';

export class Agent {
  readonly model: string;
  readonly systemPrompt: string | undefined;
  readonly maxTurns: number;
  readonly temperature: number | undefined;
  readonly maxTokens: number | undefined;
  readonly streaming: boolean;
  sessionId: string;

  // Callbacks
  onToken: ((token: string) => void) | undefined;
  onToolCall: ((name: string, args: Record<string, unknown>) => void) | undefined;
  onTurn: ((turnNum: number, message: ChatMessage) => void) | undefined;
  onError: ((error: Error) => void) | undefined;

  private messages: ChatMessage[] = [];
  private turnCount = 0;
  private totalUsage: UsageInfo = { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 };
  private client: EdgeCrabClient;
  private interrupted = false;

  constructor(options: AgentOptions = {}) {
    this.model = options.model ?? 'anthropic/claude-sonnet-4-20250514';
    this.systemPrompt = options.systemPrompt;
    this.maxTurns = options.maxTurns ?? 50;
    this.temperature = options.temperature;
    this.maxTokens = options.maxTokens;
    this.streaming = options.streaming ?? false;
    this.sessionId = options.sessionId ?? randomUUID();

    this.onToken = options.onToken;
    this.onToolCall = options.onToolCall;
    this.onTurn = options.onTurn;
    this.onError = options.onError;

    const baseUrl = options.baseUrl ?? process.env.EDGECRAB_BASE_URL ?? 'http://127.0.0.1:8642';
    const apiKey = options.apiKey ?? process.env.EDGECRAB_API_KEY;

    this.client = new EdgeCrabClient({
      baseUrl,
      apiKey,
      timeout: options.timeout ? options.timeout * 1000 : undefined,
      maxRetries: options.maxRetries,
    });

    if (this.systemPrompt) {
      this.messages.push({ role: 'system', content: this.systemPrompt });
    }
  }

  // ── Interrupt ───────────────────────────────────────────────────

  /** Signal the agent to stop after the current turn. */
  interrupt(): void {
    this.interrupted = true;
  }

  /** Clear the interrupt flag so the agent can continue. */
  clearInterrupt(): void {
    this.interrupted = false;
  }

  /** Whether the agent has been interrupted. */
  get isInterrupted(): boolean {
    return this.interrupted;
  }

  // ── Chat ────────────────────────────────────────────────────────

  /** Send a message and return the assistant's text reply. Maintains history. */
  async chat(message: string): Promise<string> {
    if (this.interrupted) throw new InterruptedError();
    if (this.turnCount >= this.maxTurns) throw new MaxTurnsExceededError(this.maxTurns);

    this.messages.push({ role: 'user', content: message });
    this.turnCount++;

    if (this.streaming && this.onToken) {
      return this.chatStreaming();
    }

    const resp = await this.client.createCompletion(this.messages, {
      model: this.model,
      temperature: this.temperature,
      maxTokens: this.maxTokens,
    });

    const assistantMsg = this.extractResponse(resp);
    this.accumulateUsage(resp.usage);
    this.onTurn?.(this.turnCount, assistantMsg);

    return assistantMsg.content;
  }

  private async chatStreaming(): Promise<string> {
    const collected: string[] = [];

    for await (const chunk of this.client.streamCompletion(this.messages, {
      model: this.model,
      temperature: this.temperature,
      maxTokens: this.maxTokens,
    })) {
      if (this.interrupted) break;
      for (const choice of chunk.choices) {
        if (choice.delta.content) {
          collected.push(choice.delta.content);
          this.onToken?.(choice.delta.content);
        }
      }
    }

    const fullText = collected.join('');
    const assistantMsg: ChatMessage = { role: 'assistant', content: fullText };
    this.messages.push(assistantMsg);
    this.onTurn?.(this.turnCount, assistantMsg);

    return fullText;
  }

  // ── Run ─────────────────────────────────────────────────────────

  /** Run a full agent conversation. Returns a structured AgentResult. */
  async run(
    message: string,
    options?: { conversationHistory?: ChatMessage[] },
  ): Promise<AgentResult> {
    if (options?.conversationHistory) {
      for (const msg of options.conversationHistory) {
        this.messages.push(msg);
      }
    }

    let response: string;
    let wasInterrupted = false;
    let wasMaxTurnsExceeded = false;

    try {
      response = await this.chat(message);
    } catch (err) {
      if (err instanceof InterruptedError) {
        response = this.messages.length > 0 ? this.messages[this.messages.length - 1].content : '';
        wasInterrupted = true;
      } else if (err instanceof MaxTurnsExceededError) {
        response = this.messages.length > 0 ? this.messages[this.messages.length - 1].content : '';
        wasMaxTurnsExceeded = true;
      } else {
        throw err;
      }
    }

    return {
      response,
      messages: [...this.messages],
      sessionId: this.sessionId,
      model: this.model,
      turnsUsed: this.turnCount,
      finishedNaturally: !wasInterrupted && !wasMaxTurnsExceeded,
      interrupted: wasInterrupted,
      maxTurnsExceeded: wasMaxTurnsExceeded,
      usage: { ...this.totalUsage },
    };
  }

  // ── Stream ──────────────────────────────────────────────────────

  /** Stream response tokens as an async iterable. */
  async *stream(message: string): AsyncGenerator<string> {
    if (this.interrupted) throw new InterruptedError();
    if (this.turnCount >= this.maxTurns) throw new MaxTurnsExceededError(this.maxTurns);

    this.messages.push({ role: 'user', content: message });
    this.turnCount++;
    const collected: string[] = [];

    for await (const chunk of this.client.streamCompletion(this.messages, {
      model: this.model,
      temperature: this.temperature,
      maxTokens: this.maxTokens,
    })) {
      if (this.interrupted) break;
      for (const choice of chunk.choices) {
        if (choice.delta.content) {
          collected.push(choice.delta.content);
          yield choice.delta.content;
        }
      }
    }

    this.messages.push({ role: 'assistant', content: collected.join('') });
  }

  // ── Conversation management ─────────────────────────────────────

  /** Manually inject a message into the conversation history. */
  addMessage(role: ChatMessage['role'], content: string): void {
    this.messages.push({ role, content });
  }

  /** Reset conversation state for a new session. */
  reset(): void {
    this.messages = [];
    this.turnCount = 0;
    this.totalUsage = { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 };
    this.sessionId = randomUUID();
    this.interrupted = false;
    if (this.systemPrompt) {
      this.messages.push({ role: 'system', content: this.systemPrompt });
    }
  }

  // ── Conversation persistence ────────────────────────────────────

  /** Export the current conversation state as a serializable object. */
  exportConversation(): ExportedConversation {
    return {
      sessionId: this.sessionId,
      model: this.model,
      messages: [...this.messages],
      turnCount: this.turnCount,
      usage: { ...this.totalUsage },
    };
  }

  /** Restore a conversation state from a previously exported object. */
  importConversation(data: ExportedConversation): void {
    this.sessionId = data.sessionId ?? this.sessionId;
    this.messages = [...(data.messages ?? [])];
    this.turnCount = data.turnCount ?? 0;
    if (data.usage) this.totalUsage = { ...data.usage };
  }

  /** Create a fork of this agent with an independent copy of the conversation. */
  clone(): Agent {
    const newAgent = new Agent({
      model: this.model,
      systemPrompt: this.systemPrompt,
      maxTurns: this.maxTurns,
      temperature: this.temperature,
      maxTokens: this.maxTokens,
      streaming: this.streaming,
      onToken: this.onToken,
      onToolCall: this.onToolCall,
      onTurn: this.onTurn,
      onError: this.onError,
    });
    newAgent.importConversation(this.exportConversation());
    newAgent.sessionId = randomUUID();
    return newAgent;
  }

  // ── Introspection ───────────────────────────────────────────────

  /** Current conversation history (copy). */
  getMessages(): ChatMessage[] {
    return [...this.messages];
  }

  /** Number of user turns completed. */
  getTurnCount(): number {
    return this.turnCount;
  }

  /** Accumulated token usage. */
  getUsage(): UsageInfo {
    return { ...this.totalUsage };
  }

  /** List available models from the server. */
  async listModels(): Promise<ModelInfo[]> {
    return this.client.listModels();
  }

  /** Check server health. */
  async health(): Promise<HealthResponse> {
    return this.client.health();
  }

  // ── Internal ────────────────────────────────────────────────────

  private extractResponse(resp: ChatCompletionResponse): ChatMessage {
    const msg: ChatMessage = resp.choices?.[0]?.message ?? { role: 'assistant', content: '' };
    this.messages.push(msg);
    return msg;
  }

  private accumulateUsage(usage?: UsageInfo): void {
    if (usage) {
      this.totalUsage.prompt_tokens += usage.prompt_tokens;
      this.totalUsage.completion_tokens += usage.completion_tokens;
      this.totalUsage.total_tokens += usage.total_tokens;
    }
  }
}
