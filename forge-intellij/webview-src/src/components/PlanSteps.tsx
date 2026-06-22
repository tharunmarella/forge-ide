import type { PlanStep } from '@/types'

export function PlanSteps({ steps }: { steps: PlanStep[] }) {
  if (!steps.length) return null
  return (
    <div style={{
      background: 'var(--bg-card)',
      border: '1px solid var(--border)',
      borderRadius: 'var(--radius)',
      padding: '8px 10px',
      marginBottom: '8px',
      fontSize: '12px',
    }}>
      <p style={{
        fontSize: '10px', fontWeight: 600, letterSpacing: '0.06em',
        textTransform: 'uppercase', color: 'var(--fg-muted)', marginBottom: '6px',
      }}>
        Plan
      </p>
      <div style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
        {steps.map((s) => {
          const isDone    = s.status === 'done'
          const isActive  = s.status === 'in_progress'
          const isFailed  = s.status === 'failed'
          const icon = isDone ? '✓' : isActive ? '→' : isFailed ? '✗' : '○'
          const color = isActive ? 'var(--accent)'
                      : isFailed  ? 'var(--danger)'
                      : isDone    ? 'var(--green)'
                      : 'var(--fg-dim)'
          return (
            <div key={s.number} style={{ display: 'flex', gap: '7px', alignItems: 'flex-start' }}>
              <span style={{
                fontFamily: 'var(--mono)', fontSize: '10px', color,
                flexShrink: 0, width: '12px', textAlign: 'right', paddingTop: '1px',
              }}>
                {icon}
              </span>
              <span style={{
                color: isDone ? 'var(--fg-muted)' : isActive ? 'var(--accent)' : isFailed ? 'var(--danger)' : 'var(--fg)',
                textDecoration: isDone ? 'line-through' : 'none',
                fontWeight: isActive ? 500 : 400,
                flex: 1,
              }}>
                {s.description}
              </span>
            </div>
          )
        })}
      </div>
    </div>
  )
}
