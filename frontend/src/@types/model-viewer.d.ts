// Copyright © 2026 Jalapeno Labs

import type { ModelViewerElement } from '@google/model-viewer'
import type { DetailedHTMLProps, HTMLAttributes } from 'react'

// Types `<model-viewer>` in JSX. The package registers the element with the DOM's tag map
// but not with React's, and module augmentation only merges into interfaces, so these
// cannot be `type`s. Only the attributes the Studio viewer sets are listed.
declare module 'react' {
  namespace JSX {
    interface IntrinsicElements {
      'model-viewer': DetailedHTMLProps<HTMLAttributes<ModelViewerElement>, ModelViewerElement> & {
        src?: string
        alt?: string
        'camera-controls'?: boolean
        'touch-action'?: string
        'interaction-prompt'?: string
        'shadow-intensity'?: string
      }
    }
  }
}
