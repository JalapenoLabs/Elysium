// Copyright © 2026 Jalapeno Labs

import type { StudioItem } from '../../api/routes/studioRoutes'

// Core
import { useTranslation } from 'react-i18next'
import { shallowEqual } from 'react-redux'

// Redux
import { selectLatestSessionsByStudioItemId } from '../../store/codingSessionsSlice'
import { useAppSelector } from '../../store/hooks'

// User interface
import { Alert } from '@heroui/react'
import { RestoreStudioItemButton } from './RestoreStudioItemButton'
import { StudioConversation } from './StudioConversation'
import { StudioItemActions } from './StudioItemActions'
import { StudioItemHeader } from './StudioItemHeader'
import { StudioSplit } from './StudioSplit'
import { StudioStage } from './StudioStage'

// Misc
import { isSessionWorking } from './studioListing'
import { useStudioLayout } from './useStudioLayout'

type Props = {
  item: StudioItem
}

// One item: the stage and the conversation side by side, or stacked on narrow screens. A
// deleted item is read-only under a banner with Restore.
export function StudioItemWorkspace(props: Props) {
  const { t } = useTranslation('studio')
  const item = props.item
  const isDeleted = Boolean(item.deletedAt)
  const { layout, isStacked, changeMode, applyResize } = useStudioLayout()
  const latestSessions = useAppSelector(selectLatestSessionsByStudioItemId, shallowEqual)
  const isWorking = isSessionWorking(latestSessions[item.id])

  return <div className='flex h-full flex-col'>
    <StudioItemHeader
      item={item}
      isReadOnly={isDeleted}
      layoutMode={layout.mode}
      onLayoutModeChange={changeMode}
      actions={<StudioItemActions item={item} />}
    />

    {isDeleted && <Alert status='warning' className='shrink-0 rounded-none'>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Title>{t('item.deletedTitle')}</Alert.Title>
        <Alert.Description>{t('item.deletedBody')}</Alert.Description>
      </Alert.Content>
      <RestoreStudioItemButton item={item} />
    </Alert>}

    {item.pullError && !isDeleted && <Alert status='danger' className='shrink-0 rounded-none'>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Description>{t('item.pullError', { reason: item.pullError })}</Alert.Description>
      </Alert.Content>
    </Alert>}

    <div className='min-h-0 flex-1'>
      <StudioSplit
        layout={layout}
        isStacked={isStacked}
        onResize={applyResize}
        preview={<StudioStage
          item={item}
          isReadOnly={isDeleted}
          isWorking={isWorking}
        />}
        chat={<StudioConversation
          item={item}
          isReadOnly={isDeleted}
        />}
      />
    </div>
  </div>
}
