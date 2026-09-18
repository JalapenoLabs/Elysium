// Copyright © 2026 Jalapeno Labs

import type { ActionItem } from '../../api/routes/actionItemRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button } from '@heroui/react'
import { LuUndo2 } from 'react-icons/lu'

// Misc
import { useActionItemActions } from './useActionItemActions'

type Props = {
  item: ActionItem
}

// Brings a deleted item back, with its projects, initiatives, and history.
export function RestoreActionItemButton(props: Props) {
  const { t } = useTranslation('actionItems')
  const actions = useActionItemActions()
  const [ isRestoring, setIsRestoring ] = useState(false)

  async function restore() {
    setIsRestoring(true)
    try {
      await actions.restore(props.item)
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
