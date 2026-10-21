// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { isHttpsHostUrl } from './browserFormSchema'

describe('isHttpsHostUrl', () => {
  it('accepts an https page named by its host', () => {
    expect(isHttpsHostUrl('https://outlook.office.com/mail/')).toBe(true)
    expect(isHttpsHostUrl('https://mail.google.com/mail/u/0/#inbox')).toBe(true)
  })

  it('refuses what the API refuses', () => {
    expect(isHttpsHostUrl('http://outlook.office.com/mail/')).toBe(false)
    expect(isHttpsHostUrl('https://203.0.113.7/')).toBe(false)
    expect(isHttpsHostUrl('https://[2001:db8::1]/')).toBe(false)
    expect(isHttpsHostUrl('file:///etc/passwd')).toBe(false)
    expect(isHttpsHostUrl('not a url')).toBe(false)
  })
})
