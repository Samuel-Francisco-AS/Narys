import { invoke } from '@tauri-apps/api/core'
import type { ConversationHistoryItem, ConversationSession } from './types'

export const createSession = () => invoke<number>('create_conversation_session')
export const getSession = (sessionId: number) => invoke<ConversationSession>('get_conversation_session', { sessionId })
export const listHistory = () => invoke<ConversationHistoryItem[]>('list_conversation_history')
export const getHistorySession = (sessionId: number) => invoke<ConversationSession>('get_conversation_history_session', { sessionId })
export const closeSession = (sessionId: number) => invoke<void>('close_conversation_session', { sessionId })
export const resumeConversationSession = (targetSessionId: number, currentSessionId: number | null) =>
  invoke<ConversationSession>('resume_conversation_session', { targetSessionId, currentSessionId })
export const geminiStatus = () => invoke<{ configured: boolean; credentialStoreAvailable: boolean; cooldownMs: number }>('gemini_status')

export type ConversationProviderState = { providerId: string; configured: boolean; cooldownMs: number }
export type ConversationRoutingStatus = { routingMode: 'fixed' | 'preferred'; primary: ConversationProviderState; fallback: ConversationProviderState | null }
export const conversationRoutingStatus = () => invoke<ConversationRoutingStatus>('conversation_routing_status')
