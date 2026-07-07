/// <reference types="vite/client" />

// Declaration merging requires `interface` here, so this augments Vite's
// built-in env typing with the variables this app reads.
interface ImportMetaEnv {
  readonly VITE_API_BASE_URL?: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}
