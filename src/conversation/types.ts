export type ConversationMessage = { id: number; sessionId: number; role: string; content: string; createdAt: string }
export type ConversationSession = { id: number; createdAt: string; updatedAt: string; status: string | null; title: string | null; summaryStatus: string; summary: string | null; summaryUpdatedAt: string | null; messages: ConversationMessage[] }
export type ConversationHistoryItem = { id: number; createdAt: string; updatedAt: string; title: string; status: string; summaryStatus: string; messageCount: number; preview: string }
export type ConversationMode = 'CURRENT' | 'HISTORY_LIST' | 'HISTORY_DETAIL'
export type ConversationState = {
  sessionId: number | null
  messages: ConversationMessage[]
  draft: string
  preview: string
  assistantStreaming: boolean
  activeTaskId: number | null
  error: string | null
}
