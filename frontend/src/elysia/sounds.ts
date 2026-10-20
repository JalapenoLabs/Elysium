// Copyright © 2026 Jalapeno Labs

import type { ElysiaLight } from '../api/routes/elysiaRoutes'

// Elysia's sound grammar: one short chime for each change worth hearing about. The tones are
// synthesized with Web Audio rather than loaded, so there is nothing to fetch and nothing for
// the Content Security Policy to refuse.

export type ElysiaSound = 'attention' | 'completion' | 'failure'

// Two notes each, in hertz, played one after the other. Rising for good news and for a
// question, falling for a failure, so each is told apart without looking.
const notesBySound = {
  attention: [ 659.25, 880 ],
  completion: [ 523.25, 783.99 ],
  failure: [ 329.63, 220 ],
} as const satisfies Record<ElysiaSound, readonly [number, number]>

// How long each note sounds, and how loud. Quiet and short: a cue, not an alarm.
const NOTE_SECONDS = 0.16
const PEAK_GAIN = 0.08

// Which sound, if any, a change of Elysia's light calls for. The first light after a page
// loads is not a change, so it plays nothing.
export function soundForTransition(previous: ElysiaLight | null, next: ElysiaLight): ElysiaSound | null {
  if (previous === null || previous === next) {
    return null
  }
  if (next === 'needs-you') {
    return 'attention'
  }
  if (next === 'errored') {
    return 'failure'
  }
  if (previous === 'running' && next === 'done') {
    return 'completion'
  }
  return null
}

let audioContext: AudioContext | null = null

// Plays `sound`. Browsers keep audio muted until the person has interacted with the page;
// a chime before then is skipped, which is logged rather than raised.
export function playElysiaSound(sound: ElysiaSound) {
  try {
    audioContext ??= new AudioContext()
    const startAt = audioContext.currentTime
    notesBySound[sound].forEach((frequency, index) => {
      const context = audioContext
      if (!context) {
        return
      }
      const oscillator = context.createOscillator()
      const gain = context.createGain()
      const noteStart = startAt + index * NOTE_SECONDS

      oscillator.type = 'sine'
      oscillator.frequency.value = frequency
      gain.gain.setValueAtTime(0, noteStart)
      gain.gain.linearRampToValueAtTime(PEAK_GAIN, noteStart + 0.02)
      gain.gain.exponentialRampToValueAtTime(0.0001, noteStart + NOTE_SECONDS)

      oscillator.connect(gain).connect(context.destination)
      oscillator.start(noteStart)
      oscillator.stop(noteStart + NOTE_SECONDS)
    })
  }
  catch (error) {
    console.debug('Could not play an Elysia sound', { sound, error })
  }
}
