// Copyright © 2026 Jalapeno Labs

import type { SettingsSave } from './settingsSave'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Card, Tooltip } from '@heroui/react'
import { LuX } from 'react-icons/lu'
import { SettingsSaveButton } from './SettingsSaveButton'

type Props = {
  save: SettingsSave
  onDismiss: () => void
}

// Save, kept within reach in the bottom right while the header's Save is scrolled out of view.
export function FloatingSaveCard(props: Props) {
  const { t } = useTranslation('settings')

  return <Card
    role='region'
    aria-label={t('page.floatingSave.label')}
    className='fixed right-6 bottom-6 z-40 shadow-lg'
  >
    <Card.Content className='flex-row items-center gap-4'>
      <span className='text-sm whitespace-nowrap'>{t('page.floatingSave.unsaved')}</span>
      <div className='level-right gap-1'>
        <SettingsSaveButton save={props.save} />
        <Tooltip delay={300}>
          <Tooltip.Trigger>
            <Button
              isIconOnly
              size='sm'
              variant='ghost'
              aria-label={t('page.floatingSave.dismiss')}
              onPress={props.onDismiss}
            >
              <LuX className='size-4' aria-hidden />
            </Button>
          </Tooltip.Trigger>
          <Tooltip.Content>
            <span>{t('page.floatingSave.dismiss')}</span>
          </Tooltip.Content>
        </Tooltip>
      </div>
    </Card.Content>
  </Card>
}
