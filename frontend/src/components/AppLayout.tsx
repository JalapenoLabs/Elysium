// Copyright © 2026 Jalapeno Labs

// Core
import { Outlet } from 'react-router'

// UI
import { Taskbar } from './taskbar/Taskbar'

// Misc
import { useRealtimeSession } from '../realtime/useRealtimeSession'

// The in-app chrome: the routed page over a fixed bottom taskbar (the app has
// no topbar, by design). The main region reserves the taskbar's height so
// content never hides behind it. Wraps the protected app pages, not the
// full-screen auth screens. Mounting here also brings up the app's realtime
// stream and its session-level wiring.
export function AppLayout() {
  useRealtimeSession()

  return <>
    <main className='app-main'>
      <Outlet />
    </main>
    <Taskbar />
  </>
}
