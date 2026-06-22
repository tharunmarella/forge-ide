import { useRef, useEffect } from 'react'
import { Send, Square } from 'lucide-react'

interface Props {
  value: string
  onChange: (v: string) => void
  onSend: () => void
  onCancel: () => void
  waiting: boolean
}

export function InputBar({ value, onChange, onSend, onCancel, waiting }: Props) {
  const ref = useRef<HTMLTextAreaElement>(null)

  useEffect(() => {
    const el = ref.current
    if (!el) return
    el.style.height = 'auto'
    el.style.height = Math.min(el.scrollHeight, 160) + 'px'
  }, [value])

  function handleKey(e: React.KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      if (!waiting && value.trim()) onSend()
    }
  }

  return (
    <div style={{
      padding: '8px 10px',
      borderTop: '1px solid var(--border)',
      background: 'var(--bg)',
      flexShrink: 0,
    }}>
      <div style={{
        display: 'flex',
        alignItems: 'flex-end',
        gap: '6px',
        background: 'var(--bg-input)',
        border: '1px solid var(--border)',
        borderRadius: 'var(--radius)',
        padding: '6px 10px',
      }}>
        <textarea
          ref={ref}
          rows={1}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={handleKey}
          disabled={waiting}
          style={{
            flex: 1,
            background: 'transparent',
            border: 'none',
            outline: 'none',
            resize: 'none',
            color: 'var(--fg)',
            caretColor: 'var(--accent)',
            fontSize: '13px',
            lineHeight: '1.5',
            minHeight: '20px',
            maxHeight: '160px',
            fontFamily: 'inherit',
          }}
        />

        {waiting ? (
          <button
            onClick={onCancel}
            style={{
              background: 'none', border: 'none', cursor: 'pointer',
              color: 'var(--danger)', padding: '2px', flexShrink: 0,
              display: 'flex', alignItems: 'center',
            }}
            title="Stop"
          >
            <Square size={13} />
          </button>
        ) : (
          <button
            onClick={onSend}
            disabled={!value.trim()}
            style={{
              background: 'none', border: 'none',
              cursor: value.trim() ? 'pointer' : 'default',
              color: value.trim() ? 'var(--accent)' : 'var(--fg-dim)',
              padding: '2px', flexShrink: 0,
              display: 'flex', alignItems: 'center',
            }}
            title="Send (Enter)"
          >
            <Send size={13} />
          </button>
        )}
      </div>
    </div>
  )
}
