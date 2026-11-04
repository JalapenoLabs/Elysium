// Copyright © 2026 Jalapeno Labs

// Core
import i18next from 'i18next'

// The one place the app builds `Intl` formatters. Building one is far slower than using it, and a
// table formats hundreds of values a render, so each is built once per language and options.
// Every formatter speaks the language i18next renders in, so dates and numbers always match the
// words around them.

const dateTimeFormats = new Map<string, Intl.DateTimeFormat>()
const numberFormats = new Map<string, Intl.NumberFormat>()
const relativeTimeFormats = new Map<string, Intl.RelativeTimeFormat>()
const listFormats = new Map<string, Intl.ListFormat>()

function cached<Formatter>(cache: Map<string, Formatter>, options: object, build: (locale: string) => Formatter) {
  const locale = i18next.language
  const key = `${locale} ${JSON.stringify(options)}`
  const existing = cache.get(key)
  if (existing) {
    return existing
  }

  const formatter = build(locale)
  cache.set(key, formatter)
  return formatter
}

export function dateTimeFormat(options: Intl.DateTimeFormatOptions) {
  return cached(dateTimeFormats, options, (locale) => new Intl.DateTimeFormat(locale, options))
}

export function numberFormat(options: Intl.NumberFormatOptions) {
  return cached(numberFormats, options, (locale) => new Intl.NumberFormat(locale, options))
}

export function relativeTimeFormat(options: Intl.RelativeTimeFormatOptions) {
  return cached(relativeTimeFormats, options, (locale) => new Intl.RelativeTimeFormat(locale, options))
}

export function listFormat(options: Intl.ListFormatOptions) {
  return cached(listFormats, options, (locale) => new Intl.ListFormat(locale, options))
}
