import { useEffect, useRef, useState, useCallback } from 'react'
import type { Message, KotlinEvent, PlanStep, ToolCall } from './types'
import { sendPrompt, cancelStream } from './bridge'
import { ChatMessage } from './components/ChatMessage'
import { InputBar } from './components/InputBar'
import { LoginScreen } from './components/LoginScreen'

let msgCounter = 0
const uid = () => `m${++msgCounter}`

function toolSummary(_name: string, args?: Record<string, unknown>): string {
  if (!args) return ''
  const v = args.path ?? args.command ?? args.query ?? args.pattern ?? args.url
  if (!v) return ''
  const s = String(v)
  const parts = s.split('/')
  return parts.length > 3 ? '…/' + parts.slice(-2).join('/') : s
}

export default function App() {
  const [showLogin, setShowLogin] = useState(false)
  const [messages, setMessages]   = useState<Message[]>([])
  const [input, setInput]         = useState('')
  const [waiting, setWaiting]     = useState(false)
  const bottomRef  = useRef<HTMLDivElement>(null)
  const curIdRef   = useRef<string | null>(null)
  const dispatchRef = useRef<((ev: KotlinEvent) => void) | null>(null)

  function scrollBottom() {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' })
  }

  // ── Mutate helpers ──────────────────────────────────────────────────────────

  function ensureAiMsg(): string {
    if (curIdRef.current) return curIdRef.current
    const id = uid()
    curIdRef.current = id
    setMessages((prev) => [
      ...prev,
      { id, role: 'ai', text: '', toolCalls: [], planSteps: [], streaming: true },
    ])
    return id
  }

  function patchMsg(id: string, updater: (m: Message) => Partial<Message>) {
    setMessages((prev) =>
      prev.map((m) => (m.id === id ? { ...m, ...updater(m) } : m))
    )
  }

  // ── Handle Kotlin events ────────────────────────────────────────────────────

  const handleEvent = useCallback((ev: KotlinEvent) => {
    switch (ev.type) {

      case 'apply_theme': {
        const root = document.documentElement
        Object.entries(ev.vars).forEach(([k, v]) => root.style.setProperty(k, String(v)))
        break
      }

      case 'auth_status':
        setShowLogin(ev.show_login)
        if (!ev.show_login) curIdRef.current = null
        break

      case 'thinking':
        ensureAiMsg()
        break

      case 'text_delta': {
        const id = ensureAiMsg()
        patchMsg(id, (m) => ({ text: m.text + ev.text }))
        scrollBottom()
        break
      }

      case 'tool_start': {
        const id = ensureAiMsg()
        const detail = toolSummary(ev.tool_name, ev.arguments)
        const tc: ToolCall = {
          id: ev.tool_call_id,
          name: ev.tool_name,
          detail,
          status: 'running',
        }
        patchMsg(id, (m) => ({ toolCalls: [...m.toolCalls, tc] }))
        scrollBottom()
        break
      }

      case 'tool_end': {
        if (!curIdRef.current) break
        const id = curIdRef.current
        patchMsg(id, (m) => ({
          toolCalls: m.toolCalls.map((tc) =>
            tc.id === ev.tool_call_id
              ? { ...tc, status: ev.success ? 'done' : 'error' }
              : tc
          ),
        }))
        break
      }

      case 'plan': {
        const id = ensureAiMsg()
        patchMsg(id, () => ({ planSteps: ev.steps as PlanStep[] }))
        break
      }

      case 'done': {
        if (curIdRef.current) {
          patchMsg(curIdRef.current, () => ({ streaming: false }))
          curIdRef.current = null
        }
        setWaiting(false)
        scrollBottom()
        break
      }

      case 'error': {
        if (curIdRef.current) {
          patchMsg(curIdRef.current, (m) => ({
            text: m.text + (m.text ? '\n\n' : '') + `⚠ ${ev.error}`,
            streaming: false,
          }))
          curIdRef.current = null
        }
        setWaiting(false)
        break
      }
    }
  }, [])

  // ── Register with global queue once mounted ─────────────────────────────────

  useEffect(() => {
    dispatchRef.current = handleEvent

    // Expose to inline script
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const w = window as any
    w.__forgeDispatch = handleEvent
    w.__forgeReady    = true

    // Drain any events that arrived before React mounted
    const queue: KotlinEvent[] = w.__forgeQueue ?? []
    w.__forgeQueue = []
    queue.forEach((ev) => handleEvent(ev))

    return () => {
      w.__forgeReady    = false
      w.__forgeDispatch = null
    }
  }, [handleEvent])

  // ── Send ────────────────────────────────────────────────────────────────────

  function send() {
    const text = input.trim()
    if (!text || waiting) return
    setInput('')
    setWaiting(true)
    curIdRef.current = null
    setMessages((prev) => [
      ...prev,
      { id: uid(), role: 'user', text, toolCalls: [], planSteps: [], streaming: false },
    ])
    sendPrompt(text)
    scrollBottom()
  }

  function cancel() {
    cancelStream()
    if (curIdRef.current) {
      patchMsg(curIdRef.current, () => ({ streaming: false }))
      curIdRef.current = null
    }
    setWaiting(false)
  }

  // ── Render ──────────────────────────────────────────────────────────────────

  if (showLogin) {
    return (
      <div style={{ height: '100%', background: 'var(--bg)', display: 'flex', flexDirection: 'column' }}>
        <LoginScreen />
      </div>
    )
  }

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column', overflow: 'hidden', background: 'var(--bg)' }}>
      {/* Message list */}
      <div style={{ flex: 1, overflowY: 'auto', padding: '12px 14px 4px' }}>

        {messages.length === 0 && (
          <div style={{
            height: '100%', display: 'flex', flexDirection: 'column',
            alignItems: 'center', justifyContent: 'center', gap: '8px',
            color: 'var(--fg-muted)',
          }}>
            <div style={{
              width: '32px', height: '32px', borderRadius: '8px',
              background: 'var(--accent)', display: 'flex',
              alignItems: 'center', justifyContent: 'center',
              color: '#fff', fontWeight: 700, fontSize: '15px',
            }}>
              F
            </div>
            <p style={{ fontSize: '12px' }}>How can I help?</p>
          </div>
        )}

        {messages.map((msg) => (
          <ChatMessage key={msg.id} msg={msg} />
        ))}

        <div ref={bottomRef} />
      </div>

      <InputBar
        value={input}
        onChange={setInput}
        onSend={send}
        onCancel={cancel}
        waiting={waiting}
      />
    </div>
  )
}
