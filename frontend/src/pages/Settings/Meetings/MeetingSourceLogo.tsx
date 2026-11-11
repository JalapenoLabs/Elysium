// Copyright © 2026 Jalapeno Labs

// User interface
import { LuAudioLines } from 'react-icons/lu'
import { ConnectorLogo } from '../Connectors/ConnectorLogo'

// Misc
import { connectorKindOf } from './meetingsPresentation'

type Props = {
  sourceKind: string
  // Its size, as Tailwind's `size-*`.
  className?: string
}

// A meeting source's logo: its connector kind's, or a plain recording mark for a source that is no
// connector this build knows, such as an upload. Decorative, as the source's name sits beside it.
export function MeetingSourceLogo(props: Props) {
  const kind = connectorKindOf(props.sourceKind)
  if (!kind) {
    return <LuAudioLines aria-hidden className={`shrink-0 opacity-70 ${props.className ?? 'size-5'}`} />
  }
  return <ConnectorLogo kind={kind} className={props.className} />
}
