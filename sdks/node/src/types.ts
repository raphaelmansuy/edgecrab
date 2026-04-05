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
