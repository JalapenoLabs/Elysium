// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useRef } from 'react'

type Props = {
  // What is being heard, from the player's audio graph (`useMeetingPlayer`), which owns it for as
  // long as its media element lives; null until audio alone first plays.
  analyser: AnalyserNode | null
  isPlaying: boolean
}

// The waveform a recording with audio alone plays over (docs/meeting-review.md, The player): what
// is being heard, drawn live as it plays, so it needs nothing of the recording but what the player
// already streams. Paused, or before it first plays, it lies flat through the middle. It draws in
// its box's text color, one point per device pixel, and again whenever its box changes size, as
// the player docks and floats.
export function PlayerWaveform(props: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const { analyser, isPlaying } = props

  useEffect(() => {
    const canvas = canvasRef.current
    const drawing = canvas?.getContext('2d')
    if (!canvas || !drawing) {
      console.debug('The meeting player\'s waveform has nothing to draw on')
      return undefined
    }
    const isLive = isPlaying && analyser !== null
    const samples = new Uint8Array(analyser?.fftSize ?? 1)
    let frame = 0

    const draw = () => {
      const scale = window.devicePixelRatio || 1
      const width = Math.round(canvas.clientWidth * scale)
      const height = Math.round(canvas.clientHeight * scale)
      if (canvas.width !== width || canvas.height !== height) {
        canvas.width = width
        canvas.height = height
      }
      if (isLive) {
        analyser.getByteTimeDomainData(samples)
      }
      else {
        // 128 is silence in a byte waveform.
        samples.fill(128)
      }
      const middle = height / 2
      drawing.clearRect(0, 0, width, height)
      drawing.lineWidth = 2 * scale
      drawing.strokeStyle = getComputedStyle(canvas).color
      drawing.beginPath()
      for (let x = 0; x <= width; x += 1) {
        const sample = samples[Math.min(samples.length - 1, Math.floor((x / width) * samples.length))]
        const y = middle + ((sample - 128) / 128) * middle
        if (x) {
          drawing.lineTo(x, y)
        }
        else {
          drawing.moveTo(0, y)
        }
      }
      drawing.stroke()
      if (isLive) {
        frame = requestAnimationFrame(draw)
      }
    }

    draw()
    // Live, every frame redraws at the new size anyway.
    const observer = new ResizeObserver(() => {
      if (!isLive) {
        draw()
      }
    })
    observer.observe(canvas)
    return () => {
      cancelAnimationFrame(frame)
      observer.disconnect()
    }
  }, [ analyser, isPlaying ])

  return <canvas
    ref={canvasRef}
    aria-hidden
    // Clear of the controls along the bottom, which stay the recording's own.
    className='pointer-events-none absolute top-3 left-4 h-[calc(100%-3.75rem)] w-[calc(100%-2rem)] text-accent'
  />
}
