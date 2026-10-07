// Copyright © 2026 Jalapeno Labs

import type { RefObject } from 'react'

// Core
import { useEffect, useEffectEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Lib
import { ModelViewerElement } from '@google/model-viewer'

// User interface
import { Spinner } from '@heroui/react'

// Misc
import { keepToOwnOrigin } from './modelViewerUrls'

// Importing the package registers `<model-viewer>`; this keeps every load it makes on the
// app's own origin, so its decoders never come from Google's CDN (see modelViewerUrls.ts).
// The page imports this module lazily, so three.js loads only with the first model shown.
ModelViewerElement.mapURLs((url) => keepToOwnOrigin(url, window.location.href))

type Props = {
  src: string
  name: string
  // The element, for Annotate to capture the current view and camera orbit.
  viewerRef: RefObject<ModelViewerElement | null>
  // Told whether the model loaded, since Annotate needs a shown view to freeze.
  onLoadStateChange?: (state: 'loaded' | 'failed') => void
}

type LoadState = 'loading' | 'loaded' | 'failed'

// One `.glb`, orbitable with the pointer or touch.
export function StudioModelViewer(props: Props) {
  const { t } = useTranslation('studio')
  const [ loadState, setLoadState ] = useState<LoadState>('loading')
  const viewerRef = props.viewerRef
  // Read at the time of the event, so a new callback each render does not re-attach the
  // listeners below.
  const reportLoadState = useEffectEvent((state: 'loaded' | 'failed') => {
    props.onLoadStateChange?.(state)
  })

  useEffect(() => {
    const viewer = viewerRef.current
    if (!viewer) {
      console.debug('StudioModelViewer has no element to watch')
      return undefined
    }

    setLoadState('loading')
    const onLoad = () => {
      setLoadState('loaded')
      reportLoadState('loaded')
    }
    const onError = (event: Event) => {
      console.debug('The 3D viewer could not show a model', { src: props.src, event })
      setLoadState('failed')
      reportLoadState('failed')
    }
    viewer.addEventListener('load', onLoad)
    viewer.addEventListener('error', onError)
    return () => {
      viewer.removeEventListener('load', onLoad)
      viewer.removeEventListener('error', onError)
    }
  }, [ viewerRef, props.src ])

  return <div className='relative size-full'>
    <model-viewer
      ref={viewerRef}
      src={props.src}
      alt={t('viewer.label', { name: props.name })}
      camera-controls
      touch-action='pan-y'
      interaction-prompt='none'
      shadow-intensity='1'
      className='block size-full'
    />
    {loadState === 'loading' && <div className='pointer-events-none absolute inset-0 grid place-items-center'>
      <Spinner />
    </div>}
    {loadState === 'failed' && <div className='absolute inset-0 grid place-items-center p-6'>
      <p className='max-w-sm text-center text-sm opacity-70'>{t('viewer.loadError')}</p>
    </div>}
  </div>
}
