export type ConversationMessage = { id: number; sessionId: number; role: string; content: string; createdAt: string }
export type ConversationSession = { id: number; messages: ConversationMessage[] }
export type ConversationState = {
  sessionId: number | null
  messages: ConversationMessage[]
  draft: string
  preview: string
  assistantStreaming: boolean
  activeTaskId: number | null
  error: string | null
}
