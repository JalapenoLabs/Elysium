// Copyright © 2026 Jalapeno Labs

// Drive path helpers: paths are provider-relative, '/'-separated, with the
// empty string as the Drive root. Kept free of URL concerns; encoding happens
// at the request layer.

export function joinPath(parent: string, name: string): string {
  if (!parent) {
    return name
  }
  return `${parent}/${name}`
}

export function parentPath(path: string): string {
  const lastSlash = path.lastIndexOf('/')
  if (lastSlash === -1) {
    return ''
  }
  return path.slice(0, lastSlash)
}

// Breadcrumb segments: each with its display name and the full path to it.
export function pathSegments(path: string): { name: string, path: string }[] {
  if (!path) {
    return []
  }

  const segments: { name: string, path: string }[] = []
  let accumulated = ''
  for (const name of path.split('/')) {
    accumulated = joinPath(accumulated, name)
    segments.push({ name, path: accumulated })
  }
  return segments
}
