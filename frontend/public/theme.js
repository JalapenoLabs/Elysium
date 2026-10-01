// Copyright © 2026 Jalapeno Labs

// Applies the saved theme before the first paint so dark mode never flashes light.
// Mirrors src/theme/themePreference.ts, which takes over once React loads.
//
// A file rather than an inline script so the page's Content Security Policy can forbid
// inline scripts outright. index.html loads it without `defer`, so it still runs
// before the body renders. The block keeps its bindings out of the global scope.
{
  let preference = 'system'
  try {
    preference = localStorage.getItem('elysium.theme') || 'system'
  }
  catch {
    // Storage can be unavailable, as in some private windows; follow the system setting.
  }
  const theme = preference === 'dark' || preference === 'light'
    ? preference
    : (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light')
  const root = document.documentElement
  root.classList.add(theme)
  root.dataset.theme = theme
  root.style.colorScheme = theme
}
