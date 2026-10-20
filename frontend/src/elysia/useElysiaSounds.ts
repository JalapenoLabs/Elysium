// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useRef } from 'react'

// Redux
import { useAppSelector } from '../store/hooks'
import { selectElysia } from '../store/elysiaSlice'

// Misc
import { playElysiaSound, soundForTransition } from './sounds'

// Plays Elysia's sounds as her light changes: a chime when she needs a person, another when
// running work finishes, and a low one when a turn fails. Mounted once, by the app shell.
export function useElysiaSounds() {
  const light = useAppSelector((state) => selectElysia(state)?.light ?? null)
  const previousLight = useRef(light)

  useEffect(() => {
    if (light === null) {
      return
    }
    const sound = soundForTransition(previousLight.current, light)
    previousLight.current = light
    if (sound) {
      playElysiaSound(sound)
    }
  }, [ light ])
}
