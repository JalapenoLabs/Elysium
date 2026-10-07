// Copyright © 2026 Jalapeno Labs

import type { CSSProperties, ReactNode } from 'react'

// Core
import { useEffect, useRef } from 'react'

// The splash behind the account pages: light, shards, waves, and rings. Purely decorative;
// the look and every animation live in `src/theme/authSplash.css`.
//
// The layers follow the pointer at different depths, so the scene shifts like parallax and the
// shards tilt toward it. The pointer's position is eased toward each frame and written to CSS
// variables on the root, so moving the mouse never re-renders React.

// How much of the remaining distance to the pointer the scene covers each frame. Low enough
// that the scene drifts after the pointer rather than snapping to it.
const POINTER_EASING = 0.06

// Below this the scene has caught up, and the animation frame loop stops until the pointer moves.
const POINTER_SETTLED = 0.001

type Shard = {
  points: string
  gradient: 'cool' | 'warm' | 'deep'
  opacity: number
  // Where it floats to and how far it turns, so no two move alike.
  drift: [number, number, number]
  delay: number
}

// Low-poly shards framing the card from the corners, in a 1600 by 900 box that is cropped
// to fill the screen. The middle stays clear for the card.
const shards: Shard[] = [
  { points: '0,0 420,0 180,260', gradient: 'cool', opacity: 0.55, drift: [ 14, 10, 3 ], delay: 0 },
  { points: '180,260 420,0 560,190', gradient: 'deep', opacity: 0.45, drift: [ -8, 12, -2 ], delay: 0.08 },
  { points: '0,0 180,260 0,420', gradient: 'deep', opacity: 0.5, drift: [ 10, -6, 2 ], delay: 0.12 },
  { points: '0,420 180,260 260,560', gradient: 'warm', opacity: 0.35, drift: [ 12, -10, -3 ], delay: 0.2 },
  { points: '1600,0 1180,0 1420,300', gradient: 'warm', opacity: 0.55, drift: [ -14, 12, -3 ], delay: 0.05 },
  { points: '1180,0 1040,170 1420,300', gradient: 'cool', opacity: 0.4, drift: [ 10, 8, 2 ], delay: 0.15 },
  { points: '1600,0 1420,300 1600,460', gradient: 'deep', opacity: 0.5, drift: [ -10, -8, 3 ], delay: 0.1 },
  { points: '0,900 360,900 120,600', gradient: 'warm', opacity: 0.5, drift: [ 12, -14, 3 ], delay: 0.18 },
  { points: '120,600 360,900 520,700', gradient: 'cool', opacity: 0.4, drift: [ -10, -8, -2 ], delay: 0.25 },
  { points: '1600,900 1220,900 1480,600', gradient: 'cool', opacity: 0.55, drift: [ -12, -12, -3 ], delay: 0.1 },
  { points: '1480,600 1220,900 1080,720', gradient: 'warm', opacity: 0.4, drift: [ 10, -10, 2 ], delay: 0.22 },
  { points: '1600,460 1480,600 1600,900', gradient: 'deep', opacity: 0.45, drift: [ -8, 10, 2 ], delay: 0.3 },
]

// Gentle sine-like curves across the full width, offset in height and phase.
const waves = [
  {
    path: 'M-120,560 C200,480 400,640 700,560 S1200,480 1720,580',
    color: 'var(--splash-cyan)',
    opacity: 0.55,
  },
  {
    path: 'M-120,620 C250,540 450,700 800,620 S1250,520 1720,640',
    color: 'var(--splash-violet)',
    opacity: 0.45,
  },
  {
    path: 'M-120,660 C320,600 520,740 900,660 S1350,580 1720,700',
    color: 'var(--splash-magenta)',
    opacity: 0.35,
  },
]

const gradientStops = {
  cool: [ 'var(--splash-cyan)', 'var(--splash-blue)' ],
  warm: [ 'var(--splash-amber)', 'var(--splash-magenta)' ],
  deep: [ 'var(--splash-violet)', 'var(--splash-blue)' ],
} as const satisfies Record<Shard['gradient'], readonly [string, string]>

// Follows the pointer with the element's `--pointer-x` and `--pointer-y`, each from -1 to 1,
// unless the person asked for less motion or has no hovering pointer.
function usePointerParallax() {
  const rootRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const root = rootRef.current
    const wantsStillness = window.matchMedia('(prefers-reduced-motion: reduce)').matches
    const canHover = window.matchMedia('(hover: hover)').matches
    if (!root || wantsStillness || !canHover) {
      return undefined
    }

    const target = { x: 0, y: 0 }
    const current = { x: 0, y: 0 }
    let frame = 0

    function step() {
      current.x += (target.x - current.x) * POINTER_EASING
      current.y += (target.y - current.y) * POINTER_EASING
      root?.style.setProperty('--pointer-x', current.x.toFixed(4))
      root?.style.setProperty('--pointer-y', current.y.toFixed(4))

      const settled = Math.abs(target.x - current.x) < POINTER_SETTLED
        && Math.abs(target.y - current.y) < POINTER_SETTLED
      frame = settled
        ? 0
        : requestAnimationFrame(step)
    }

    function follow(event: PointerEvent) {
      target.x = (event.clientX / window.innerWidth) * 2 - 1
      target.y = (event.clientY / window.innerHeight) * 2 - 1
      if (!frame) {
        frame = requestAnimationFrame(step)
      }
    }

    // Leaving the window lets the scene drift back to rest.
    function rest() {
      target.x = 0
      target.y = 0
      if (!frame) {
        frame = requestAnimationFrame(step)
      }
    }

    window.addEventListener('pointermove', follow, { passive: true })
    document.documentElement.addEventListener('pointerleave', rest)
    return () => {
      window.removeEventListener('pointermove', follow)
      document.documentElement.removeEventListener('pointerleave', rest)
      cancelAnimationFrame(frame)
    }
  }, [])

  return rootRef
}

// One depth of the scene. Layers lean toward the pointer, nearer ones more than deeper ones.
type LayerProps = {
  depth: number
  tilt?: boolean
  children: ReactNode
}

function Layer(props: LayerProps) {
  return <div
    className={props.tilt
      ? 'auth-splash-layer auth-splash-layer-tilt'
      : 'auth-splash-layer'}
    style={{ '--depth': props.depth } as CSSProperties}
  >
    {props.children}
  </div>
}

export function AuthSplash() {
  const rootRef = usePointerParallax()

  return <div ref={rootRef} className='auth-splash' aria-hidden='true'>
    <Layer depth={0.35}>
      <div className='auth-splash-light auth-splash-light-a' />
      <div className='auth-splash-light auth-splash-light-b' />
      <div className='auth-splash-light auth-splash-light-c' />
    </Layer>
    <Layer depth={-0.2}>
      <div className='auth-splash-beam' />
    </Layer>

    <Layer depth={1} tilt>
    <svg className='auth-splash-shards' viewBox='0 0 1600 900' preserveAspectRatio='xMidYMid slice'>
      <defs>
        {Object.entries(gradientStops).map(([ name, [ from, to ]]) => <linearGradient
          key={name}
          id={`auth-splash-${name}`}
          x1='0'
          y1='0'
          x2='1'
          y2='1'
        >
          <stop offset='0%' style={{ stopColor: from }} />
          <stop offset='100%' style={{ stopColor: to, stopOpacity: 0.15 }} />
        </linearGradient>)}
      </defs>

      {shards.map((shard) => <g key={shard.points}>
        <polygon
          className='auth-splash-shard'
          points={shard.points}
          fill={`url(#auth-splash-${shard.gradient})`}
          style={{
            '--shard-opacity': shard.opacity,
            '--shard-dx': `${shard.drift[0]}px`,
            '--shard-dy': `${shard.drift[1]}px`,
            '--shard-turn': `${shard.drift[2]}deg`,
            'animationDelay': `${shard.delay}s, ${shard.delay + 1.4}s`,
          } as CSSProperties}
        />
        <polygon className='auth-splash-edge' points={shard.points} />
      </g>)}

      {waves.map((wave, index) => <path
        key={wave.path}
        className='auth-splash-wave'
        d={wave.path}
        style={{
          stroke: wave.color,
          opacity: wave.opacity,
          animationDelay: `${0.3 + index * 0.2}s, ${2.7 + index * 0.2}s`,
        }}
      />)}
    </svg>
    </Layer>

    <Layer depth={0.5}>
      <div className='auth-splash-rings'>
        <div className='auth-splash-ring' />
        <div className='auth-splash-ring' />
        <div className='auth-splash-ring' />
      </div>
    </Layer>
    <div className='auth-splash-grain' />
  </div>
}
