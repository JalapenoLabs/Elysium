// Copyright © 2026 Jalapeno Labs

// noVNC 1.7 exports its client from the package root (`exports: ./core/rfb.js`), while the
// DefinitelyTyped declarations still name the older `lib/rfb` path. The class is the same.
declare module '@novnc/novnc' {
  import NoVncClient from '@novnc/novnc/lib/rfb'

  // The package's own export is a default class, so its declaration is too.
  // eslint-disable-next-line import/no-default-export
  export default NoVncClient
}
