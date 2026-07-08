// Copyright © 2026 Jalapeno Labs

// Core
import { Outlet } from 'react-router'

// UI
import { NavBar } from './NavBar'

// The in-app chrome: the NavBar over the routed page. Wraps the protected app
// pages (home, about, settings), not the full-screen auth screens.
export function AppLayout() {
  return <>
    <NavBar />
    <Outlet />
  </>
}
