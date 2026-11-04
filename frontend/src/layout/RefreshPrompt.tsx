// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../store/hooks'
import { refreshPromptDismissed, selectIsRefreshOffered } from '../store/versionSlice'

// User interface
import { Button, Card, Tooltip } from '@heroui/react'
import { LuRefreshCw, LuX } from 'react-icons/lu'

// Offers a refresh once Elysium runs a build other than the one this page loaded, in the
// content's bottom left corner, clear of the toasts and the floating Save in the bottom right.
// Dismissing it lasts until a later build arrives.
export function RefreshPrompt() {
  const { t } = useTranslation('navigation')
  const dispatch = useAppDispatch()
  const isOffered = useAppSelector(selectIsRefreshOffered)
  // The reload takes a moment to begin, so the button shows it is under way.
  const [ isRefreshing, setIsRefreshing ] = useState(false)

  if (!isOffered) {
    return null
  }

  return <Card
    role='status'
    className='absolute right-4 bottom-4 left-4 z-30 shadow-lg sm:right-auto'
  >
    <Card.Content className='flex-row items-center gap-3'>
      <LuRefreshCw className='size-4 shrink-0 text-accent' aria-hidden />
      <span className='min-w-0 flex-1 text-sm'>{
        t('updatePrompt.message')
      }</span>
      <div className='level-right shrink-0 gap-1'>
        <Button
          size='sm'
          isPending={isRefreshing}
          onPress={() => {
            setIsRefreshing(true)
            window.location.reload()
          }}
        >
          <span>{
            t('updatePrompt.refresh')
          }</span>
        </Button>
        <Tooltip delay={300}>
          <Button
            isIconOnly
            size='sm'
            variant='ghost'
            aria-label={t('updatePrompt.dismiss')}
            onPress={() => dispatch(refreshPromptDismissed())}
          >
            <LuX className='size-4' aria-hidden />
          </Button>
          <Tooltip.Content>
            <span>{
              t('updatePrompt.dismiss')
            }</span>
          </Tooltip.Content>
        </Tooltip>
      </div>
    </Card.Content>
  </Card>
}
