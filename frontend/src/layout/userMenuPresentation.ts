// Copyright © 2026 Jalapeno Labs

// Up to two letters for an avatar: the first letters of the first and last words of a name.
export function getInitials(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean)
  if (!words.length) {
    return '?'
  }
  const first = [ ...words[0] ][0] ?? ''
  const last = words.length > 1
    ? [ ...words[words.length - 1] ][0] ?? ''
    : ''
  return `${first}${last}`.toUpperCase()
}
