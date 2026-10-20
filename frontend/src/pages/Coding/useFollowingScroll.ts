// Copyright © 2026 Jalapeno Labs

// Core
import { useLayoutEffect, useRef } from 'react'

// How close to the bottom, in pixels, still counts as "following" the conversation.
const FOLLOW_THRESHOLD_PX = 48

// Keeps a scrolling conversation on its latest message as messages arrive, until the reader
// scrolls up to read; returning to the bottom follows again. `contentVersion` is anything
// that changes when content is added, such as the number of events shown.
export function useFollowingScroll<Element extends HTMLElement>(contentVersion: number) {
  const scrollRef = useRef<Element>(null)
  // Starts true so opening a conversation lands on its latest message.
  const isFollowingRef = useRef(true)

  useLayoutEffect(() => {
    const container = scrollRef.current
    if (container && isFollowingRef.current) {
      container.scrollTop = container.scrollHeight
    }
  }, [ contentVersion ])

  function onScroll() {
    const container = scrollRef.current
    if (!container) {
      return
    }
    const distanceFromBottom = container.scrollHeight - container.scrollTop - container.clientHeight
    isFollowingRef.current = distanceFromBottom <= FOLLOW_THRESHOLD_PX
  }

  return { scrollRef, onScroll } as const
}
