// Copyright © 2026 Jalapeno Labs

import type { IconType } from 'react-icons'
import type { ConnectorKind } from '../../../api/routes/connectorRoutes'

// User interface
import { LuAudioLines } from 'react-icons/lu'
import { PiMicrosoftOutlookLogoFill, PiMicrosoftTeamsLogoFill, PiSlackLogoFill } from 'react-icons/pi'
import { SiConfluence, SiGithub, SiGmail, SiJetbrains, SiJira } from 'react-icons/si'

type ConnectorLogo = {
  icon: IconType
  // The brand's own color, which is each company's rather than the theme's. A brand whose
  // mark is black follows the theme's text color, so it reads in dark mode too.
  className: string
}

// Every kind's logo, bundled rather than loaded from the provider, since the web app's pages
// load nothing from another origin. simple-icons no longer carries Microsoft's, Slack's,
// Fireflies', or YouTrack's marks, so those come from Phosphor, YouTrack shows its maker's
// mark, and Fireflies a plain waveform.
export const connectorLogos = {
  jira: { icon: SiJira, className: 'text-[#2684ff]' },
  confluence: { icon: SiConfluence, className: 'text-[#2684ff]' },
  github: { icon: SiGithub, className: '' },
  outlook: { icon: PiMicrosoftOutlookLogoFill, className: 'text-[#0f78d4]' },
  gmail: { icon: SiGmail, className: 'text-[#ea4335]' },
  teams: { icon: PiMicrosoftTeamsLogoFill, className: 'text-[#7b83eb]' },
  slack: { icon: PiSlackLogoFill, className: 'text-[#e01e5a]' },
  fireflies: { icon: LuAudioLines, className: 'text-[#e8457c]' },
  fireflies_browser: { icon: LuAudioLines, className: 'text-[#e8457c]' },
  youtrack: { icon: SiJetbrains, className: '' },
} as const satisfies Record<ConnectorKind, ConnectorLogo>
