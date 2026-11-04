// Copyright © 2026 Jalapeno Labs

import type { ConnectorKind } from '../../../api/routes/connectorRoutes'

// Misc
import { connectorLogos } from './connectorLogos'

type Props = {
  kind: ConnectorKind
  // Its size, as Tailwind's `size-*`.
  className?: string
}

// A kind's logo, in its brand's color. Decorative: the kind's name always sits beside it.
export function ConnectorLogo(props: Props) {
  const logo = connectorLogos[props.kind]
  const Icon = logo.icon

  return <Icon
    aria-hidden
    className={`shrink-0 ${logo.className} ${props.className ?? 'size-5'}`}
  />
}
