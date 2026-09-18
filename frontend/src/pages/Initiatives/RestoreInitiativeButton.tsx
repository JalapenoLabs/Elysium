// Copyright © 2026 Jalapeno Labs

import type { Initiative } from '../../api/routes/initiativeRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button } from '@heroui/react'
import { LuUndo2 } from 'react-icons/lu'

// Misc
import { useInitiativeActions } from './useInitiativeActions'

type Props = {
  initiative: Initiative
}

// Brings a deleted initiative back; its items list it again.
export function RestoreInitiativeButton(props: Props) {
  const { t } = useTranslation('initiatives')
  const actions = useInitiativeActions()
  const [ isRestoring, setIsRestoring ] = useState(false)

  async function restore() {
    setIsRestoring(true)
    try {
      await actions.restore(props.initiative)
    }
    finally {
      setIsRestoring(false)
    }
  }

  return <Button size='sm' variant='outline' isPending={isRestoring} onPress={() => void restore()}>
    <LuUndo2 className='size-4' aria-hidden />
    <span>{t('list.restore')}</span>
  </Button>
}
