import { Github } from 'lucide-react'
import { startAuth } from '@/bridge'
import { useState } from 'react'

export function LoginScreen() {
  const [loading, setLoading] = useState(false)

  function handleGitHub() {
    setLoading(true)
    startAuth()
  }

  return (
    <div style={{
      height: '100%', display: 'flex', flexDirection: 'column',
      alignItems: 'center', justifyContent: 'center', gap: '20px', padding: '0 32px',
      background: 'var(--bg)',
    }}>
      <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: '10px' }}>
        <div style={{
          width: '40px', height: '40px', borderRadius: '10px',
          background: 'var(--accent)', display: 'flex', alignItems: 'center',
          justifyContent: 'center', color: '#fff', fontWeight: 700, fontSize: '18px',
        }}>
          F
        </div>
        <p style={{ fontWeight: 600, fontSize: '13px', color: 'var(--fg)' }}>Forge AI</p>
        <p style={{ fontSize: '12px', color: 'var(--fg-muted)', textAlign: 'center' }}>
          Sign in to start
        </p>
      </div>

      <button
        onClick={handleGitHub}
        disabled={loading}
        style={{
          display: 'flex', alignItems: 'center', gap: '8px',
          width: '100%', justifyContent: 'center',
          padding: '7px 16px', borderRadius: 'var(--radius)',
          background: 'var(--bg-card)', border: '1px solid var(--border)',
          color: 'var(--fg)', fontSize: '13px', cursor: 'pointer',
          opacity: loading ? 0.6 : 1, fontFamily: 'inherit',
        }}
      >
        <Github size={14} />
        {loading ? 'Opening browser…' : 'Continue with GitHub'}
      </button>

      {loading && (
        <p style={{ fontSize: '11px', color: 'var(--fg-muted)', textAlign: 'center' }}>
          Complete sign-in in your browser, then return here.
        </p>
      )}
    </div>
  )
}
