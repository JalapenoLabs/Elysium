// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useRef, useState } from 'react'

type Props = {
  value: string
  // Read by screen readers in place of the invisible field's missing visuals.
  label: string
  onChange: (value: string) => void
  onSubmit: () => void
  // Renders each typed character as a dot; the field still submits real text.
  masked?: boolean
  inputMode?: 'email' | 'text' | 'numeric'
  autoComplete?: string
  // Bump to replay the invalid-submit shake (a nonce, not a boolean, so the
  // same validation failure twice in a row still shakes twice).
  shakeNonce?: number
}

// How long the invalid-submit shake plays before the class is removed so it can
// replay on the next failure. Matches the CSS animation duration.
const SHAKE_DURATION_MS = 450

// The login gate's type-anywhere input. A real (but invisible) <input> owns
// focus for the whole screen, keeping screen readers, IMEs, paste, password
// managers, and mobile keyboards working, while the typed value renders as hero
// glyphs mid-screen. Nothing here looks like a form on purpose.
export function GhostInput(props: Props) {
  const inputRef = useRef<HTMLInputElement>(null)
  const [ isShaking, setIsShaking ] = useState(false)

  // The field owns focus for the whole screen: take it on mount and retake it
  // whenever it slips away, so "just start typing" is always true. The retake
  // waits a frame so it never fights a click that is still being dispatched.
  useEffect(() => {
    inputRef.current?.focus()
  }, [])

  useEffect(() => {
    if (!props.shakeNonce) {
      return undefined
    }
    setIsShaking(true)
    const timer = setTimeout(() => setIsShaking(false), SHAKE_DURATION_MS)
    return () => clearTimeout(timer)
  }, [ props.shakeNonce ])

  const glyphs = props.masked
    ? '•'.repeat(props.value.length)
    : props.value

  // The line auto-scales: hero-sized while short, shrinking as the value grows
  // so long emails never overflow the stage. The floor keeps it readable.
  const glyphCount = Math.max(glyphs.length, 8)
  const glyphFontSize = `max(2rem, min(8vw, ${(120 / glyphCount).toFixed(2)}vw))`

  return <div
    className={`ghost-input ${isShaking ? 'ghost-input--shake' : ''}`}
    onPointerDown={() => inputRef.current?.focus()}
  >
    <input
      ref={inputRef}
      className='ghost-input__field'
      type={props.masked ? 'password' : 'text'}
      inputMode={props.inputMode}
      autoComplete={props.autoComplete}
      aria-label={props.label}
      value={props.value}
      onChange={(event) => props.onChange(event.currentTarget.value)}
      onBlur={() => {
        requestAnimationFrame(() => inputRef.current?.focus())
      }}
      onKeyDown={(event) => {
        if (event.key === 'Enter') {
          event.preventDefault()
          props.onSubmit()
        }
      }}
    />
    <div
      className='ghost-input__glyphs'
      style={{ fontSize: glyphFontSize }}
      aria-hidden='true'
    >
      { glyphs.split('').map((character, index) => (
        <span key={index} className='ghost-input__glyph'>
          { character === ' ' ? '\u00A0' : character }
        </span>
      )) }
      <span className='ghost-input__caret' />
    </div>
  </div>
}
