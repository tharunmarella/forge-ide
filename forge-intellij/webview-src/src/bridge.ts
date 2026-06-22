/**
 * Bridge between React and Kotlin (JCEF).
 * Kotlin → React : window.postMessage({ type, ...payload }, '*')
 * React  → Kotlin : window.sendMessageToKotlin(JSON.stringify({ action, ...payload }))
 */

export function sendToKotlin(action: string, payload?: Record<string, unknown>) {
  const msg = JSON.stringify({ action, ...payload })
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  ;(window as any).sendMessageToKotlin?.(msg)
}

export function sendPrompt(text: string) {
  sendToKotlin('send_message', { message: text })
}

export function cancelStream() {
  sendToKotlin('cancel')
}

export function startAuth() {
  sendToKotlin('start_auth')
}
