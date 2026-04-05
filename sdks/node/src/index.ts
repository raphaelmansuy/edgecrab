/**
 * EdgeCrab Node.js SDK — main entry point.
 *
 * @example
 * ```ts
 * import { Agent } from 'edgecrab-sdk';
 *
 * const agent = new Agent({ model: 'anthropic/claude-sonnet-4-20250514' });
 * const reply = await agent.chat('Hello!');
 * console.log(reply);
 * ```
 */

export { Agent } from './agent.js';
export {
  EdgeCrabClient,
  EdgeCrabError,
  AuthenticationError,
  RateLimitError,
  ServerError,
  TimeoutError,
  ConnectionError,
  MaxTurnsExceededError,
  InterruptedError,
} from './client.js';
export type {
  AgentOptions,
  AgentResult,
  ChatChoice,
  ChatCompletionRequest,
  ChatCompletionResponse,
  ChatMessage,
  ClientOptions,
  ExportedConversation,
  HealthResponse,
  ModelInfo,
  StreamChunk,
  StreamChoice,
  StreamDelta,
  UsageInfo,
} from './types.js';
