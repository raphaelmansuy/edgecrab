/** Message roles. */
export declare const Role: Readonly<{
  System: 'system'
  User: 'user'
  Assistant: 'assistant'
  Tool: 'tool'
}>

/** A chat message. */
export declare class Message {
  role: string
  content: string
  tool_call_id?: string
  constructor(role: string, content: string, toolCallId?: string)
  static system(content: string): Message
  static user(content: string): Message
  static assistant(content: string): Message
  static tool(toolCallId: string, content: string): Message
  toJSON(): { role: string; content: string; tool_call_id?: string }
}

/** Tool definition input. */
export interface ToolDefinition {
  /** Unique tool name. */
  name: string
  /** Human-readable description for the LLM. */
  description: string
  /** JSON Schema for tool parameters. */
  parameters?: Record<string, unknown>
  /** Async handler function invoked when the LLM calls this tool. */
  handler: (args: Record<string, unknown>) => Promise<unknown> | unknown
}

/** A tool for the agent to call. */
export declare class Tool {
  name: string
  description: string
  parameters?: Record<string, unknown>
  handler: (args: Record<string, unknown>) => Promise<unknown> | unknown
  constructor(definition: ToolDefinition)
  /** Factory method for creating tools. */
  static create(definition: ToolDefinition): Tool
  /** Get the JSON schema for this tool (OpenAI-compatible format). */
  toSchema(): {
    type: 'function'
    function: {
      name: string
      description: string
      parameters: Record<string, unknown>
    }
  }
  /** Alias for compatibility with other SDKs/examples. */
  toFunctionSchema(): {
    type: 'function'
    function: {
      name: string
      description: string
      parameters: Record<string, unknown>
    }
  }
  /** Execute the tool handler with the given arguments. */
  execute(args: Record<string, unknown>): Promise<unknown>
}
