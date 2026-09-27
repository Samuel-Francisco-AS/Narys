import { invoke } from '@tauri-apps/api/core'
import type { ConversationSession } from './types'

export const createSession = () => invoke<number>('create_conversation_session')
export const getSession = (sessionId: number) => invoke<ConversationSession>('get_conversation_session', { sessionId })
export const closeSession = (sessionId: number) => invoke<void>('close_conversation_session', { sessionId })
export const geminiStatus = () => invoke<{ configured: boolean; credentialStoreAvailable: boolean }>('gemini_status')
