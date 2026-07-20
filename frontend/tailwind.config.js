// Copyright © 2026 Jalapeno Labs

import { heroui } from '@heroui/react'

/** @type {import('tailwindcss').Config} */
export default {
  content: [
    './index.html',
    './src/**/*.{js,ts,jsx,tsx}',
    // HeroUI ships prebuilt component styles that Tailwind must scan.
    './node_modules/@heroui/theme/dist/**/*.{js,ts,jsx,tsx}',
  ],
  theme: {
    extend: {
      // The status-color grammar (docs/ux.md). Values are CSS variables declared in
      // src/index.css so they follow the light/dark theme; the `<alpha-value>` slot
      // keeps Tailwind opacity modifiers working (e.g. `bg-status-active/15`).
      colors: {
        status: {
          'idle': 'hsl(var(--status-idle) / <alpha-value>)',
          'active': 'hsl(var(--status-active) / <alpha-value>)',
          'needs-input': 'hsl(var(--status-needs-input) / <alpha-value>)',
          'error': 'hsl(var(--status-error) / <alpha-value>)',
          'done': 'hsl(var(--status-done) / <alpha-value>)',
        },
      },
    },
  },
  darkMode: 'class',
  plugins: [ heroui() ],
}
