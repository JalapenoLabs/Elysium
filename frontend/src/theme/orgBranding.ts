// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect } from 'react'

// Redux
import { useAppSelector } from '../store/hooks'

// Converts `#RRGGBB` into the `H S% L%` triple HeroUI's CSS variables expect.
// Returns null (and logs) for anything that isn't a six-digit hex color.
export function hexToHslTriple(hex: string): string | null {
  const match = /^#([0-9a-f]{6})$/i.exec(hex.trim())
  if (!match) {
    console.debug('not a #RRGGBB color, ignoring for branding', { hex })
    return null
  }

  const value = parseInt(match[1], 16)
  const red = ((value >> 16) & 0xff) / 255
  const green = ((value >> 8) & 0xff) / 255
  const blue = (value & 0xff) / 255

  const maximum = Math.max(red, green, blue)
  const minimum = Math.min(red, green, blue)
  const lightness = (maximum + minimum) / 2
  const delta = maximum - minimum

  let hue = 0
  let saturation = 0
  if (delta !== 0) {
    saturation = delta / (1 - Math.abs(2 * lightness - 1))
    if (maximum === red) {
      hue = 60 * (((green - blue) / delta) % 6)
    }
    else if (maximum === green) {
      hue = 60 * ((blue - red) / delta + 2)
    }
    else {
      hue = 60 * ((red - green) / delta + 4)
    }
  }
  if (hue < 0) {
    hue += 360
  }

  return `${Math.round(hue)} ${Math.round(saturation * 100)}% ${Math.round(lightness * 100)}%`
}

// White-label theming: when the active org sets an accent color, it overrides
// HeroUI's primary across the app; when empty (or on sign-out/unmount) the
// Elysium default returns. The org switcher, the taskbar's active-app bar, and
// every primary button pick it up for free because they all read the variable.
export function useOrgBranding() {
  const orgs = useAppSelector((state) => state.sessionContext.orgs)
  const activeOrgId = useAppSelector((state) => state.sessionContext.activeOrgId)

  const accentColor = orgs.find((org) => org.id === activeOrgId)?.accentColor ?? ''

  useEffect(() => {
    const root = document.documentElement
    const triple = accentColor ? hexToHslTriple(accentColor) : null

    if (triple) {
      root.style.setProperty('--heroui-primary', triple)
    }
    else {
      root.style.removeProperty('--heroui-primary')
    }

    return () => {
      root.style.removeProperty('--heroui-primary')
    }
  }, [ accentColor ])
}
