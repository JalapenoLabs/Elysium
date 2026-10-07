// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

// Core
import { useState } from 'react'

type Props = {
  src: string
  alt: string
  className?: string
  loading?: 'eager' | 'lazy'
  // Shown in place of the image once it fails to load, as when there is no image at all.
  fallback: ReactNode
  // Told whether the image loaded, for a caller whose actions need its pixels.
  onLoadStateChange?: (state: 'loaded' | 'failed') => void
}

// A stored file's image. Its content comes through the API from a storage provider, which
// can be down or have lost the file; a broken image would show as an empty box, so the
// fallback takes its place. Failure is remembered per `src`, so a new file is tried afresh.
export function StudioImage(props: Props) {
  const [ failedSrc, setFailedSrc ] = useState<string | null>(null)

  if (failedSrc === props.src) {
    return <>{props.fallback}</>
  }

  return <img
    src={props.src}
    alt={props.alt}
    loading={props.loading}
    className={props.className}
    onLoad={() => props.onLoadStateChange?.('loaded')}
    onError={() => {
      console.debug('A Studio image could not be loaded', { src: props.src })
      setFailedSrc(props.src)
      props.onLoadStateChange?.('failed')
    }}
  />
}
