// EdgeCrab SDK — TypeScript wrapper types
// These are pure-JS/TS types that complement the native bindings.

/**
 * Message roles.
 */
const Role = Object.freeze({
  System: 'system',
  User: 'user',
  Assistant: 'assistant',
  Tool: 'tool',
})

/**
 * A chat message.
 */
class Message {
  /**
   * @param {string} role - One of Role.System, Role.User, Role.Assistant, Role.Tool
   * @param {string} content - The message text
   * @param {string | undefined} toolCallId - Optional tool call id for tool messages
   */
  constructor(role, content, toolCallId) {
    this.role = role
    this.content = content
    if (toolCallId) {
      this.tool_call_id = toolCallId
    }
  }

  static system(content) {
    return new Message(Role.System, content)
  }

  static user(content) {
    return new Message(Role.User, content)
  }

  static assistant(content) {
    return new Message(Role.Assistant, content)
  }

  static tool(toolCallId, content) {
    return new Message(Role.Tool, content, toolCallId)
  }

  toJSON() {
    const json = { role: this.role, content: this.content }
    if (this.tool_call_id) {
      json.tool_call_id = this.tool_call_id
    }
    return json
  }
}

/**
 * A tool definition for the agent.
 *
 * Usage:
 *   const myTool = Tool.create({
 *     name: 'get_weather',
 *     description: 'Get weather for a city',
 *     parameters: {
 *       type: 'object',
 *       properties: {
 *         city: { type: 'string', description: 'City name' }
 *       },
 *       required: ['city']
 *     },
 *     handler: async (args) => {
 *       return { temperature: 72, unit: 'F' }
 *     }
 *   })
 */
class Tool {
  /**
   * @param {ToolDefinition} definition
   */
  constructor(definition) {
    this.name = definition.name
    this.description = definition.description
    this.parameters = definition.parameters
    this.handler = definition.handler
  }

  /**
   * Factory method for creating tools.
   * @param {ToolDefinition} definition
   * @returns {Tool}
   */
  static create(definition) {
    if (!definition.name) throw new Error('Tool name is required')
    if (!definition.description) throw new Error('Tool description is required')
    if (!definition.handler) throw new Error('Tool handler is required')
    return new Tool(definition)
  }

  /**
   * Get the JSON schema for this tool (OpenAI-compatible format).
   */
  toSchema() {
    return {
      type: 'function',
      function: {
        name: this.name,
        description: this.description,
        parameters: this.parameters || { type: 'object', properties: {} },
      },
    }
  }

  /**
   * Alias for compatibility with other SDKs/examples.
   */
  toFunctionSchema() {
    return this.toSchema()
  }

  /**
   * Execute the tool handler with the given arguments.
   * @param {Record<string, unknown>} args
   * @returns {Promise<unknown>}
   */
  async execute(args) {
    return this.handler(args)
  }
}

module.exports = { Tool, Message, Role }
