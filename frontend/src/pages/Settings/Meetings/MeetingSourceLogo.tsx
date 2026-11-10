// Copyright © 2026 Jalapeno Labs

import type { MeetingSourceKind } from '../../../api/routes/meetingRoutes'

// User interface
import { LuUpload } from 'react-icons/lu'
import { ConnectorLogo } from '../Connectors/ConnectorLogo'

// Misc
import { UPLOAD_SOURCE_KIND } from '../../../api/routes/meetingRoutes'

type Props = {
  kind: MeetingSourceKind
  // Its size, as Tailwind's `size-*`.
  className?: string
}

// Where a meeting came from, as a mark: its connector kind's logo, or an upload's arrow for one a
// person uploaded. Decorative: the source's name always sits beside it.
export function MeetingSourceLogo(props: Props) {
  if (props.kind === UPLOAD_SOURCE_KIND) {
    return <LuUpload aria-hidden className={`shrink-0 opacity-70 ${props.className ?? 'size-5'}`} />
  }
  return <ConnectorLogo kind={props.kind} className={props.className} />
}
