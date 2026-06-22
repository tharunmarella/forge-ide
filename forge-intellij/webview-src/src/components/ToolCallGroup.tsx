import { useState } from 'react'
import { ChevronRight } from 'lucide-react'
import type { ToolCall } from '@/types'

function Spinner() {
  return (
    <span style={{
      display: 'inline-block', width: '7px', height: '7px', borderRadius: '50%',
      border: '1.5px solid var(--fg-dim)', borderTopColor: 'var(--accent)',
      animation: 'spin 0.7s linear infinite', flexShrink: 0,
    }} />
  )
}

function StatusDot({ status }: { status: ToolCall['status'] }) {
  if (status === 'running') return <Spinner />
  return (
    <span style={{
      display: 'inline-block', width: '7px', height: '7px', borderRadius: '50%',
      background: status === 'done' ? 'var(--green)' : 'var(--danger)',
      flexShrink: 0,
    }} />
  )
}

export function ToolCallGroup({ tools, streaming }: { tools: ToolCall[]; streaming: boolean }) {
  const [expanded, setExpanded] = useState(false)
  if (!tools.length) return null

  const lastRunning = tools.findLast?.((t) => t.status === 'running')
  const label = streaming && lastRunning
    ? lastRunning.name
    : `${tools.length} action${tools.length === 1 ? '' : 's'}`

  return (
    <div style={{ marginBottom: '6px' }}>
      {/* Summary row */}
      <button
        onClick={() => setExpanded((v) => !v)}
        style={{
          display: 'inline-flex', alignItems: 'center', gap: '4px',
          background: 'var(--bg-card)', border: '1px solid var(--border)',
          borderRadius: '4px', padding: '2px 7px',
          color: 'var(--fg-muted)', fontSize: '11px',
          cursor: 'pointer', fontFamily: 'var(--mono)',
        }}
      >
        {streaming && lastRunning && <Spinner />}
        <ChevronRight
          size={9}
          style={{
            transform: expanded ? 'rotate(90deg)' : 'none',
            transition: 'transform 0.15s',
            color: 'var(--fg-dim)',
          }}
        />
        <span style={{ color: 'var(--fg)', fontWeight: 500 }}>{label}</span>
      </button>

      {/* Expanded list */}
      {expanded && (
        <div style={{
          marginTop: '4px',
          paddingLeft: '10px',
          borderLeft: '2px solid var(--border)',
          display: 'flex',
          flexDirection: 'column',
          gap: '2px',
        }}>
          {tools.map((tc) => (
            <div key={tc.id} style={{
              display: 'flex', alignItems: 'center', gap: '6px',
              fontSize: '11px', fontFamily: 'var(--mono)',
              color: 'var(--fg-muted)', overflow: 'hidden',
            }}>
              <StatusDot status={tc.status} />
              <span style={{ color: 'var(--fg)', flexShrink: 0 }}>{tc.name}</span>
              {tc.detail && (
                <span style={{
                  color: 'var(--fg-muted)',
                  overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flex: 1,
                }}>
                  {tc.detail}
                </span>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
