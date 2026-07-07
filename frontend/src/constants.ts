// Base URL for the Rust API. Injected at build time by Vite; falls back to the
// port the backend publishes locally so the template runs with zero config.
export const API_BASE_URL = import.meta.env.VITE_API_BASE_URL || 'http://localhost:31416'
