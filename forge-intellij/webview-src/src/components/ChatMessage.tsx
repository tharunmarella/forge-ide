import { useEffect, useRef } from 'react'
import { marked } from 'marked'
import hljs from 'highlight.js/lib/core'
import langTs from 'highlight.js/lib/languages/typescript'
import langJs from 'highlight.js/lib/languages/javascript'
import langPy from 'highlight.js/lib/languages/python'
import langRust from 'highlight.js/lib/languages/rust'
import langKotlin from 'highlight.js/lib/languages/kotlin'
import langBash from 'highlight.js/lib/languages/bash'
import langJson from 'highlight.js/lib/languages/json'
import langXml from 'highlight.js/lib/languages/xml'
import langCss from 'highlight.js/lib/languages/css'
import langSql from 'highlight.js/lib/languages/sql'
import 'highlight.js/styles/atom-one-dark.css'
import type { Message } from '@/types'

hljs.registerLanguage('typescript', langTs)
hljs.registerLanguage('javascript', langJs)
hljs.registerLanguage('python', langPy)
hljs.registerLanguage('rust', langRust)
hljs.registerLanguage('kotlin', langKotlin)
hljs.registerLanguage('bash', langBash)
hljs.registerLanguage('shell', langBash)
hljs.registerLanguage('sh', langBash)
hljs.registerLanguage('json', langJson)
hljs.registerLanguage('xml', langXml)
hljs.registerLanguage('html', langXml)
hljs.registerLanguage('css', langCss)
hljs.registerLanguage('sql', langSql)
import { PlanSteps } from './PlanSteps'
import { ToolCallGroup } from './ToolCallGroup'

// Configure marked with syntax highlighting
marked.setOptions({ breaks: true, gfm: true })
const renderer = new marked.Renderer()
renderer.code = function ({ text, lang }) {
  const language = lang && hljs.getLanguage(lang) ? lang : 'plaintext'
  const highlighted = hljs.highlight(text, { language }).value
  return `<pre><code class="hljs language-${language}">${highlighted}</code></pre>`
}
marked.use({ renderer })

export function ChatMessage({ msg }: { msg: Message }) {
  const isUser = msg.role === 'user'
  const bodyRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (bodyRef.current && !isUser && msg.text) {
      // marked.parse can return Promise in v9+, use parseSync to guarantee string
      const html = marked.parse(msg.text, { async: false }) as string
      bodyRef.current.innerHTML = html
    }
  }, [msg.text, isUser])

  if (isUser) {
    return (
      <div style={{ animation: 'fadeIn 0.15s ease' }} className="mb-2 flex justify-end">
        <div
          style={{
            background: 'var(--bg-card)',
            border: '1px solid var(--border)',
            borderRadius: 'var(--radius)',
            color: 'var(--fg)',
            padding: '6px 12px',
            maxWidth: '90%',
            wordBreak: 'break-word',
            fontSize: '13px',
            lineHeight: '1.5',
          }}
        >
          {msg.text}
        </div>
      </div>
    )
  }

  // AI message
  return (
    <div style={{ animation: 'fadeIn 0.15s ease' }} className="mb-4">
      {msg.planSteps.length > 0 && <PlanSteps steps={msg.planSteps} />}

      {msg.toolCalls.length > 0 && (
        <ToolCallGroup tools={msg.toolCalls} streaming={msg.streaming} />
      )}

      {msg.text ? (
        <div ref={bodyRef} className="prose" />
      ) : msg.streaming && msg.toolCalls.length === 0 ? (
        <div
          style={{ display: 'flex', alignItems: 'center', gap: '6px', color: 'var(--fg-muted)', fontSize: '12px' }}
        >
          <span
            style={{
              display: 'inline-block', width: '8px', height: '8px', borderRadius: '50%',
              border: '1.5px solid var(--fg-muted)', borderTopColor: 'var(--accent)',
              animation: 'spin 0.7s linear infinite',
            }}
          />
          Thinking…
        </div>
      ) : null}
    </div>
  )
}
