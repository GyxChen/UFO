import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

export const AGENT_STATUS_EVENT = 'agent-status'

export type Provider = 'claude' | 'codex'
export type SessionPhase = 'running' | 'waiting' | 'error' | 'unknown' | 'completed'
export type StatusColor = 'green' | 'yellow' | 'red'

export interface SessionSnapshot {
  key: string
  provider: Provider
  session_id: string
  phase: SessionPhase
  source: 'hook' | 'process_fallback'
  confidence: number
  started_at_ms: number
  last_seen_at_ms: number
  completed_at_ms: number | null
  remove_at_ms: number | null
  is_flashing: boolean
  pid: number | null
  error_code: string | null
  last_sequence: number | null
  termination_confirmed: boolean
}

export interface StatusSnapshot {
  schema_version: number
  revision: number
  generated_at_ms: number
  sessions: SessionSnapshot[]
  counts: {
    running: number
    waiting: number
    error: number
    unknown: number
    completed: number
  }
  source_health: string
}

export function colorForSession(session: SessionSnapshot): StatusColor {
  if (session.phase === 'error') return 'red'
  if (session.phase === 'running' || session.phase === 'completed') return 'green'
  return 'yellow'
}

export function isStatusSnapshot(value: unknown): value is StatusSnapshot {
  if (!value || typeof value !== 'object') return false
  const snapshot = value as Partial<StatusSnapshot>
  return (
    snapshot.schema_version === 1 &&
    Number.isSafeInteger(snapshot.revision) &&
    snapshot.revision >= 0 &&
    typeof snapshot.generated_at_ms === 'number' &&
    Array.isArray(snapshot.sessions) &&
    snapshot.sessions.length <= 1000 &&
    snapshot.sessions.every((session) =>
      session &&
      typeof session.key === 'string' &&
      (session.provider === 'claude' || session.provider === 'codex') &&
      typeof session.session_id === 'string' &&
      typeof session.phase === 'string' &&
      typeof session.termination_confirmed === 'boolean',
    ) &&
    typeof snapshot.counts === 'object' &&
    snapshot.counts !== null
  )
}

export async function listenToAgentStatus(
  onSnapshot: (snapshot: StatusSnapshot) => void,
): Promise<UnlistenFn> {
  return listen<StatusSnapshot>(AGENT_STATUS_EVENT, (event) => {
    if (isStatusSnapshot(event.payload)) onSnapshot(event.payload)
  })
}

export function getAgentStatusSnapshot(): Promise<StatusSnapshot> {
  return invoke<StatusSnapshot>('get_status_snapshot')
}
