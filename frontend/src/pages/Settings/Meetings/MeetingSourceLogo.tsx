// Copyright © 2026 Jalapeno Labs

// User interface
import { LuAudioLines, LuUpload } from 'react-icons/lu'
import { ConnectorLogo } from '../Connectors/ConnectorLogo'

// Misc
import { UPLOAD_SOURCE_KIND } from '../../../api/routes/meetingRoutes'
import { connectorKindOf } from './meetingsPresentation'

type Props = {
  sourceKind: string
  // Its size, as Tailwind's `size-*`.
  className?: string
}

// A meeting source's logo: its connector kind's, an upload's arrow for one a person uploaded, or a
// plain recording mark for a source that is no connector this build knows. Decorative, as the
// source's name sits beside it.
export function MeetingSourceLogo(props: Props) {
  const markClassName = `shrink-0 opacity-70 ${props.className ?? 'size-5'}`
  if (props.sourceKind === UPLOAD_SOURCE_KIND) {
    return <LuUpload aria-hidden className={markClassName} />
  }
  const kind = connectorKindOf(props.sourceKind)
  if (!kind) {
    return <LuAudioLines aria-hidden className={markClassName} />
  }
  return <ConnectorLogo kind={kind} className={props.className} />
}
