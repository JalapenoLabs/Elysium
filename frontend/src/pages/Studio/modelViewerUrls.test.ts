// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { keepToOwnOrigin } from './modelViewerUrls'

const PAGE_URL = 'https://elysium.example/studio/item'

describe('keepToOwnOrigin', () => {
  it('passes a path on the app\'s own origin, as the model\'s content route is', () => {
    const contentPath = '/api/v1/studio-items/item/assets/asset/content'
    expect(keepToOwnOrigin(contentPath, PAGE_URL)).toBe(contentPath)
    expect(keepToOwnOrigin('https://elysium.example/api/x', PAGE_URL)).toBe('https://elysium.example/api/x')
  })

  it('passes a texture the loader unpacked into a blob, and inline data', () => {
    const blobUrl = 'blob:https://elysium.example/0b9a5a1e-6f0e-4b5e-9a57-1c2f3d4e5f60'
    expect(keepToOwnOrigin(blobUrl, PAGE_URL)).toBe(blobUrl)
    expect(keepToOwnOrigin('data:image/png;base64,AAAA', PAGE_URL)).toBe('data:image/png;base64,AAAA')
  })

  it('refuses the decoders on Google\'s CDN', () => {
    const decoder = 'https://www.gstatic.com/draco/versioned/decoders/1.5.6/draco_decoder.wasm'
    expect(() => keepToOwnOrigin(decoder, PAGE_URL)).toThrow(/www\.gstatic\.com/)
  })

  it('refuses another port or scheme of the same host', () => {
    expect(() => keepToOwnOrigin('https://elysium.example:8443/x', PAGE_URL)).toThrow()
    expect(() => keepToOwnOrigin('http://elysium.example/x', PAGE_URL)).toThrow()
  })

  it('refuses a blob another origin created', () => {
    expect(() => keepToOwnOrigin('blob:https://elsewhere.example/id', PAGE_URL)).toThrow()
  })
})
