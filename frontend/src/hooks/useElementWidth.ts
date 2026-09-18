// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useRef, useState } from 'react'

// The rendered width of an element, kept current as it resizes, for drawings such as charts
// that lay themselves out in pixels. Zero until the element has been measured.
export function useElementWidth<Element extends HTMLElement>() {
  const ref = useRef<Element>(null)
  const [ width, setWidth ] = useState(0)

  useEffect(() => {
    const element = ref.current
    if (!element) {
      console.debug('useElementWidth has no element to measure')
      return undefined
    }
    const observer = new ResizeObserver((entries) => {
      setWidth(entries[0]?.contentRect.width ?? 0)
    })
    observer.observe(element)
    return () => observer.disconnect()
  }, [])

  return { ref, width } as const
}
