import { defineStore } from 'pinia'
import { retryOnStartupRace } from '@/lib/startup-retry'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import {
  colorForSession,
  getAgentStatusSnapshot,
  listenToAgentStatus,
  type SessionSnapshot,
  type StatusColor,
  type StatusSnapshot,
} from '../lib/agent-status'

let stopListener: (() => void) | undefined
let stopHookListener: (() => void) | undefined

export const useAgentStatusStore = defineStore('agent-status', {
  state: () => ({
    snapshot: null as StatusSnapshot | null,
    connection: 'starting' as 'starting' | 'connected' | 'degraded',
    lastRevision: 0,
    started: false,
  }),

  getters: {
    sessions: (state): SessionSnapshot[] => state.snapshot?.sessions ?? [],
    sessionColor: () => (session: SessionSnapshot): StatusColor => colorForSession(session),
    hasSessions: (state) => (state.snapshot?.sessions.length ?? 0) > 0,
  },

  actions: {
    applySnapshot(snapshot: StatusSnapshot) {
      if (snapshot.revision < this.lastRevision) return
      this.snapshot = snapshot
      this.lastRevision = snapshot.revision
      this.connection = snapshot.source_health === 'healthy' ? 'connected' : 'degraded'
    },

    /** Removes one visible status light and applies the returned snapshot immediately. */
    async dismissSession(key: string) {
      const snapshot = await invoke<StatusSnapshot>('dismiss_status_session', { key })
      this.applySnapshot(snapshot)
    },

    async start() {
      if (this.started) return
      this.started = true
      try {
        stopListener = await listenToAgentStatus((snapshot) => this.applySnapshot(snapshot))
        // 首帧 IPC 可能早于后端 .manage()：重试几次，别让一次竞态退化成「连不上」。
        this.applySnapshot(await retryOnStartupRace(getAgentStatusSnapshot))
      } catch (error) {
        this.connection = 'degraded'
        console.warn('agent status channel unavailable', error)
      }
      stopHookListener = await listen('status-hook-event', (event) => {
        console.log('[status-hook-event]', event.payload)
      })
    },

    stop() {
      stopListener?.()
      stopListener = undefined
      stopHookListener?.()
      stopHookListener = undefined
      this.started = false
    },
  },
})
