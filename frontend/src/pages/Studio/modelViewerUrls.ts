// Copyright © 2026 Jalapeno Labs

// The 3D viewer loads nothing from another origin. model-viewer fetches its Draco and KTX2
// decoders from Google's CDN when a model uses those compressions, and runs them as blob
// workers with WebAssembly; the web app's Content Security Policy allows none of that
// (docs/security.md), and Elysium's own export never compresses (docs/studio.md). Every URL
// the viewer's loaders ask for passes through this, so a compressed model fails to preview
// in every environment, the development stack included, instead of reaching the CDN.

// Returns `url` when it stays on the app's own origin: a path on it, a blob it created, or
// inline data. Anything else throws, which the loader reports as the model failing to load.
export function keepToOwnOrigin(url: string, baseUrl: string) {
  const resolved = new URL(url, baseUrl)
  if (resolved.protocol === 'data:') {
    return url
  }
  // A blob URL's origin is the origin of the document that created it.
  if (resolved.origin === new URL(baseUrl).origin) {
    return url
  }
  throw new Error(`The 3D viewer refused to load ${resolved.origin}, which is not Elysium's own origin`)
}
