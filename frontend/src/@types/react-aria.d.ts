// Copyright © 2026 Jalapeno Labs

import type { NavigateOptions } from 'react-router'

// Types the `routerOptions` every React Aria and HeroUI link takes as React Router's own, so a
// link can carry what `navigate` does, such as the state that lets it leave a page without the
// leave prompt (`LEAVE_WITHOUT_ASKING`). `AppShell`'s `RouterProvider` hands them to `navigate`.
// Module augmentation only merges into interfaces, so this cannot be a `type`.
declare module 'react-aria-components' {
  interface RouterConfig {
    routerOptions: NavigateOptions
  }
}
