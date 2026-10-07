// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Checkbox } from '@heroui/react'

type Props = {
  onPermanentChange: (isPermanent: boolean) => void
}

// The Delete confirmation's body: what a delete does, and the unchecked box that makes it
// permanent.
export function DeleteStudioItemMessage(props: Props) {
  const { t } = useTranslation('studio')
  const [ isPermanent, setIsPermanent ] = useState(false)

  return <div>
    <p className='relaxed'>{t('delete.body')}</p>
    <Checkbox
      isSelected={isPermanent}
      onChange={(isSelected) => {
        setIsPermanent(isSelected)
        props.onPermanentChange(isSelected)
      }}
    >
      <Checkbox.Content className='items-start'>
        <Checkbox.Control className='mt-0.5 shrink-0'>
          <Checkbox.Indicator />
        </Checkbox.Control>
        <span className='text-sm'>{t('delete.permanently')}</span>
      </Checkbox.Content>
    </Checkbox>
    {isPermanent && <p className='mt-2 ml-7 text-sm text-danger'>{t('delete.permanentlyWarning')}</p>}
  </div>
}
