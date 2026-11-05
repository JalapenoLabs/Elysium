// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { readMarkdownImage, readMarkdownLink } from './markdownUrls'

const { origin } = window.location

describe('readMarkdownLink', () => {
  it('keeps a link within the document as it is', () => {
    expect(readMarkdownLink('#user-content-fn-1')).toEqual({ kind: 'fragment', href: '#user-content-fn-1' })
  })

  it('routes paths and absolute links back to Elysium in the app', () => {
    expect(readMarkdownLink('/knowledge/ask?tab=1#top')).toEqual({
      kind: 'internal',
      href: '/knowledge/ask?tab=1#top',
    })
    expect(readMarkdownLink(`${origin}/projects`)).toEqual({ kind: 'internal', href: '/projects' })
  })

  it('treats another origin as external, however it is spelled', () => {
    const external = { kind: 'external', href: 'https://example.com/' }

    expect(readMarkdownLink('https://example.com/')).toEqual(external)
    expect(readMarkdownLink('//example.com/')).toEqual({
      kind: 'external',
      href: `${window.location.protocol}//example.com/`,
    })
    expect(readMarkdownLink('\\\\example.com/').kind).toBe('external')
    expect(readMarkdownLink('/\\example.com/').kind).toBe('external')
  })

  it('leaves mail links external', () => {
    expect(readMarkdownLink('mailto:someone@example.com').kind).toBe('external')
  })

  it('has nothing to follow once the address was blanked', () => {
    expect(readMarkdownLink('')).toEqual({ kind: 'none' })
    expect(readMarkdownLink(undefined)).toEqual({ kind: 'none' })
  })
})

describe('readMarkdownImage', () => {
  it('loads an image from Elysium', () => {
    expect(readMarkdownImage('/api/v1/attachments/1?size=2')).toEqual({
      kind: 'internal',
      src: '/api/v1/attachments/1?size=2',
    })
    expect(readMarkdownImage(`${origin}/logo.png`)).toEqual({ kind: 'internal', src: '/logo.png' })
  })

  it('names the host of an image on another origin', () => {
    expect(readMarkdownImage('https://images.example.com/cat.png')).toEqual({
      kind: 'remote',
      href: 'https://images.example.com/cat.png',
      host: 'images.example.com',
    })
    expect(readMarkdownImage('//images.example.com/cat.png').kind).toBe('remote')
  })

  it('has nothing to load once the source was blanked', () => {
    expect(readMarkdownImage('')).toEqual({ kind: 'none' })
    expect(readMarkdownImage(undefined)).toEqual({ kind: 'none' })
  })
})
