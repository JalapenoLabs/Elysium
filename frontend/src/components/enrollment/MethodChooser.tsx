// Copyright © 2026 Jalapeno Labs

import type { AuthMethodKind } from '../../auth/methods/types'

// Core
import { useTranslation } from 'react-i18next'

// UI
import { Card, CardBody } from '@heroui/react'

// Misc
import { authMethodByKind } from '../../auth/methods/registry'

type Props = {
  // Which methods to offer, in display order.
  methods: readonly AuthMethodKind[]
  // Disable the cards while the identity is incomplete or a ceremony is running.
  isDisabled?: boolean
  onChoose: (kind: AuthMethodKind) => void
}

// The sign-in-method chooser: one pressable card per method, titled and described
// from the shared registry (`method.*` i18n keys). Shared by first-run claim and
// invite acceptance so the two present identically. See docs/auth-model.md.
export function MethodChooser(props: Props) {
  const { t } = useTranslation('auth')

  return <div className='mt-3 flex flex-col gap-3'>
    { props.methods.map((kind) => {
      const meta = authMethodByKind[kind]
      return <Card
        key={kind}
        isPressable
        isDisabled={props.isDisabled}
        className='w-full'
        onPress={() => props.onChoose(kind)}
      >
        <CardBody className='items-start'>
          <h3 className='text-medium'>
            <strong>{ t(meta.titleKey) }</strong>
          </h3>
          <p className='mt-1 text-sm opacity-70'>{ t(meta.blurbKey) }</p>
        </CardBody>
      </Card>
    }) }
  </div>
}
