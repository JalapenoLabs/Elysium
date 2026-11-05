// Copyright © 2026 Jalapeno Labs

import type { KnowledgePageSummary } from '../../../api/routes/pageRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectPageTree } from '../../../store/pagesSlice'

// User interface
import { Button } from '@heroui/react'
import { LuArchiveRestore } from 'react-icons/lu'

// Misc
import { usePageActions } from './usePageActions'

type Props = {
  page: KnowledgePageSummary
}

// A deleted page's way back, once the Undo in its toast has gone: the deletion's Undo, on its page.
export function RestorePageButton(props: Props) {
  const { t } = useTranslation('knowledge')
  const tree = useAppSelector(selectPageTree)
  const actions = usePageActions(tree)
  const [ isRestoring, setIsRestoring ] = useState(false)

  async function restore() {
    setIsRestoring(true)
    await actions.restore(props.page)
    setIsRestoring(false)
  }

  return <Button variant='outline' isPending={isRestoring} onPress={() => void restore()}>
    <LuArchiveRestore className='size-4' aria-hidden />
    <span>{t('pages.restore')}</span>
  </Button>
}
