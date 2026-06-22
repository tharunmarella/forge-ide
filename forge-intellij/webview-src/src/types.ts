export type ToolStatus = 'running' | 'done' | 'error'

export interface ToolCall {
  id: string
  name: string
  detail?: string
  status: ToolStatus
}

export interface PlanStep {
  number: number
  description: string
  status: 'pending' | 'in_progress' | 'done' | 'failed'
}

export type MessageRole = 'user' | 'ai'

export interface Message {
  id: string
  role: MessageRole
  // Markdown text content (streamed in)
  text: string
  toolCalls: ToolCall[]
  planSteps: PlanStep[]
  // While streaming
  streaming: boolean
}

// Events sent from Kotlin → React via window.postMessage
export type KotlinEvent =
  | { type: 'text_delta';   text: string }
  | { type: 'tool_start';   tool_call_id: string; tool_name: string; arguments?: Record<string, unknown> }
  | { type: 'tool_end';     tool_call_id: string; tool_name: string; success: boolean; result_summary?: string }
  | { type: 'plan';         steps: PlanStep[] }
  | { type: 'done';         answer: string; total_time_ms: number }
  | { type: 'error';        error: string }
  | { type: 'thinking';     step_type: string; message: string }
  | { type: 'auth_status';  show_login: boolean }
  | { type: 'apply_theme';  vars: Record<string, string> }
