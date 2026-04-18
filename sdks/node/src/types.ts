/** Type definitions for the EdgeCrab Node.js SDK. */

export interface ChatMessage {
  role: 'system' | 'user' | 'assistant' | 'tool';
  content: string;
  name?: string;
  tool_call_id?: string;
}

export interface ChatCompletionRequest {
  model: string;
  messages: ChatMessage[];
  temperature?: number;
  max_tokens?: number;
  stream?: boolean;
  tools?: Record<string, unknown>[];
}

export interface UsageInfo {
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
}

export interface ChatChoice {
  index: number;
  message: ChatMessage;
  finish_reason: string | null;
}

export interface ChatCompletionResponse {
  id: string;
  object: string;
  created: number;
  model: string;
  choices: ChatChoice[];
  usage?: UsageInfo;
}

export interface StreamDelta {
  role?: string;
  content?: string;
}

export interface StreamChoice {
  index: number;
  delta: StreamDelta;
  finish_reason: string | null;
}

export interface StreamChunk {
  id: string;
  object: string;
  created: number;
  model: string;
  choices: StreamChoice[];
}

export interface ModelInfo {
  id: string;
  object: string;
  created?: number;
  owned_by?: string;
}

export interface HealthResponse {
  status: string;
  version?: string;
}

export interface AgentOptions {
  model?: string;
  baseUrl?: string;
  apiKey?: string;
  systemPrompt?: string;
  maxTurns?: number;
  temperature?: number;
  maxTokens?: number;
  timeout?: number;
  sessionId?: string;
  streaming?: boolean;
  maxRetries?: number;
  onToken?: (token: string) => void;
  onToolCall?: (name: string, args: Record<string, unknown>) => void;
  onTurn?: (turnNum: number, message: ChatMessage) => void;
  onError?: (error: Error) => void;
}

export interface AgentResult {
  response: string;
  messages: ChatMessage[];
  sessionId: string;
  model: string;
  turnsUsed: number;
  finishedNaturally: boolean;
  interrupted: boolean;
  maxTurnsExceeded: boolean;
  usage: UsageInfo;
}

export interface ClientOptions {
  baseUrl?: string;
  apiKey?: string;
  timeout?: number;
  maxRetries?: number;
  retryBaseDelay?: number;
}

export interface ExportedConversation {
  sessionId: string;
  model: string;
  messages: ChatMessage[];
  turnCount: number;
  usage: UsageInfo;
}

// ─── Convenience types (parity with native SDK) ─────────────────────

/** Message roles. */
export const Role = Object.freeze({
  System: 'system' as const,
  User: 'user' as const,
  Assistant: 'assistant' as const,
  Tool: 'tool' as const,
});

/** A chat message with factory helpers. */
export class Message {
  role: ChatMessage['role'];
  content: string;

  constructor(role: ChatMessage['role'], content: string) {
    this.role = role;
    this.content = content;
  }

  static system(content: string): Message {
    return new Message('system', content);
  }

  static user(content: string): Message {
    return new Message('user', content);
  }

  static assistant(content: string): Message {
    return new Message('assistant', content);
  }

  static tool(content: string): Message {
    return new Message('tool', content);
  }

  toJSON(): ChatMessage {
    return { role: this.role, content: this.content };
  }
}

/** Tool definition input. */
export interface ToolDefinition {
  name: string;
  description: string;
  parameters?: Record<string, unknown>;
  handler: (args: Record<string, unknown>) => Promise<unknown> | unknown;
}

/** A tool that the agent can call. */
export class Tool {
  name: string;
  description: string;
  parameters?: Record<string, unknown>;
  handler: (args: Record<string, unknown>) => Promise<unknown> | unknown;

  constructor(definition: ToolDefinition) {
    this.name = definition.name;
    this.description = definition.description;
    this.parameters = definition.parameters;
    this.handler = definition.handler;
  }

  /** Factory method. */
  static create(definition: ToolDefinition): Tool {
    if (!definition.name) throw new Error('Tool name is required');
    if (!definition.description) throw new Error('Tool description is required');
    if (!definition.handler) throw new Error('Tool handler is required');
    return new Tool(definition);
  }

  /** OpenAI-compatible function schema. */
  toSchema(): {
    type: 'function';
    function: { name: string; description: string; parameters: Record<string, unknown> };
  } {
    return {
      type: 'function',
      function: {
        name: this.name,
        description: this.description,
        parameters: this.parameters ?? { type: 'object', properties: {} },
      },
    };
  }

  /** Execute the tool handler. */
  async execute(args: Record<string, unknown>): Promise<unknown> {
    return this.handler(args);
  }
}
