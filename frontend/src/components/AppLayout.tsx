// Copyright © 2026 Jalapeno Labs

// Core
import { Outlet } from 'react-router'

// UI
import { Taskbar } from './taskbar/Taskbar'
import { CommandPalette } from './palette/CommandPalette'
import { DriveSearchModal } from '../drive/DriveSearchModal'

// Misc
import { useRealtimeSession } from '../realtime/useRealtimeSession'
import { useNavigationPaletteEntries } from './palette/useNavigationPaletteEntries'
import { useProjectPaletteEntries } from './palette/useProjectPaletteEntries'
import { useCoreWorkspacePanels } from '../workspace/useCoreWorkspacePanels'
import { useWorkspacePaletteEntries } from '../workspace/useWorkspacePaletteEntries'
import { useProjectsBootstrap } from '../hooks/useProjectsBootstrap'
import { useOrgBranding } from '../theme/orgBranding'

// The in-app chrome: the routed page over a fixed bottom taskbar (the app has
// no topbar, by design). The main region reserves the taskbar's height so
// content never hides behind it. Wraps the protected app pages, not the
// full-screen auth screens. Mounting here also brings up the app's realtime
// stream, its session-level wiring, the active org's white-label accent, and
// the ctrl/cmd+P command palette with the core navigation entries registered.
export function AppLayout() {
  useRealtimeSession()
  useOrgBranding()
  useProjectsBootstrap()
  useNavigationPaletteEntries()
  useProjectPaletteEntries()
  useCoreWorkspacePanels()
  useWorkspacePaletteEntries()

  return <>
    <main className='app-main'>
      <Outlet />
    </main>
    <Taskbar />
    <CommandPalette />
    <DriveSearchModal />
  </>
}
