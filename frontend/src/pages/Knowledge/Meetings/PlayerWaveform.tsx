// Copyright © 2026 Jalapeno Labs

import type { RefObject } from 'react'

// Core
import { useEffect, useRef } from 'react'

type AudioGraph = {
  context: AudioContext
  analyser: AnalyserNode
}

// A media element's sound passes through Web Audio once it is listened to, and it can be given a
// source node only once in its life, so the graph is kept per element for as long as it lives.
const graphByMedia = new WeakMap<HTMLMediaElement, AudioGraph>()

// The analyser between `media` and the speakers, made the first time it is asked for. Its sound
// still reaches the speakers through it, since a source node takes it from the element.
function graphOf(media: HTMLMediaElement): AudioGraph | null {
  const known = graphByMedia.get(media)
  if (known) {
    return known
  }
  try {
    const context = new AudioContext()
    const analyser = context.createAnalyser()
    analyser.fftSize = 2048
    context.createMediaElementSource(media).connect(analyser)
    analyser.connect(context.destination)
    const graph = { context, analyser }
    graphByMedia.set(media, graph)
    return graph
  }
  catch (error) {
    // Without Web Audio the recording still plays, over black.
    console.debug('The meeting player could not listen to its recording for a waveform', { error })
    return null
  }
}

type Props = {
  mediaRef: RefObject<HTMLVideoElement | null>
  isPlaying: boolean
}

// The waveform a recording with audio alone plays over (docs/meeting-review.md, The player): what
// is being heard, drawn live as it plays, so it needs nothing of the recording but what the
// player already streams. Paused, it lies flat. It draws in the text's color of its box.
export function PlayerWaveform(props: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const { mediaRef, isPlaying } = props

  useEffect(() => {
    const media = mediaRef.current
    const canvas = canvasRef.current
    const drawing = canvas?.getContext('2d')
    if (!media || !canvas || !drawing) {
      return undefined
    }
    const graph = graphOf(media)
    if (!graph) {
      return undefined
    }
    // A browser starts Web Audio suspended until someone has pressed something on the page;
    // playing is that press.
    if (isPlaying) {
      graph.context.resume().catch((error: unknown) => {
        console.debug('The meeting player\'s waveform could not resume its audio', { error })
      })
    }
    const samples = new Uint8Array(graph.analyser.fftSize)
    let frame = 0

    const draw = () => {
      const scale = window.devicePixelRatio || 1
      const width = Math.round(canvas.clientWidth * scale)
      const height = Math.round(canvas.clientHeight * scale)
      if (canvas.width !== width || canvas.height !== height) {
        canvas.width = width
        canvas.height = height
      }
      if (isPlaying) {
        graph.analyser.getByteTimeDomainData(samples)
      }
      else {
        // 128 is silence in a byte waveform.
        samples.fill(128)
      }
      drawing.clearRect(0, 0, width, height)
      drawing.lineWidth = 2 * scale
      drawing.strokeStyle = getComputedStyle(canvas).color
      drawing.beginPath()
      const step = width / (samples.length - 1)
      for (let index = 0; index < samples.length; index += 1) {
        const y = (samples[index] / 255) * height
        if (index) {
          drawing.lineTo(index * step, y)
        }
        else {
          drawing.moveTo(0, y)
        }
      }
      drawing.stroke()
      if (isPlaying) {
        frame = requestAnimationFrame(draw)
      }
    }

    draw()
    return () => cancelAnimationFrame(frame)
  }, [ mediaRef, isPlaying ])

  return <canvas
    ref={canvasRef}
    aria-hidden
    // Clear of the controls along the bottom, which stay the recording's own.
    className='pointer-events-none absolute top-3 left-4 h-[calc(100%-3.75rem)] w-[calc(100%-2rem)] text-accent'
  />
}
