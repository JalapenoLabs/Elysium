// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// UI
import { Button, Input, Popover, PopoverContent, PopoverTrigger, Select, SelectItem } from '@heroui/react'

// Misc
import { useCurrentUser } from '../../auth/useCurrentUser'
import { setMyStatus } from '../../api/routes/userRoutes'
import { effectiveStatusOf } from './presence'
import { ManualStatusKind } from '../../proto/generated/user_pb'

// Presence is automatic (derived from live connections server-side); this
// control only sets or clears the manual override that supersedes it. The user
// never picks online/away/offline by hand. See docs/identity.md.

// How long an override lasts before presence reverts to automatic. `0` lets the
// server apply its 24h default, so that stays the initially-selected option.
const durationSecondsByKey = {
  hour1: 3600n,
  hour4: 14400n,
  day1: 0n,
  week1: 604800n,
} as const

type DurationKey = keyof typeof durationSecondsByKey

// The app-chrome status chip: shows the effective status (manual override when
// active, derived presence otherwise) and opens a panel to set or clear the
// override. Reads the shared `currentUser` slice; `setMyStatus` responses keep
// it (and every other consumer) current.
export function StatusControl() {
  const { t } = useTranslation('common')
  const user = useCurrentUser()

  const [ isOpen, setIsOpen ] = useState(false)
  const [ isBusy, setIsBusy ] = useState(false)
  const [ customText, setCustomText ] = useState('')
  const [ durationKey, setDurationKey ] = useState<DurationKey>('day1')

  if (!user) {
    return null
  }

  const status = effectiveStatusOf(user)
  const label = status.customText || t(status.labelKey)

  async function applyStatus(kind: ManualStatusKind, text: string = '') {
    setIsBusy(true)
    try {
      await setMyStatus(kind, text, durationSecondsByKey[durationKey])
      setIsOpen(false)
      setCustomText('')
    }
    catch (error) {
      console.debug('failed to update the manual status', { error })
    }
    finally {
      setIsBusy(false)
    }
  }

  return <Popover placement='top-end' isOpen={isOpen} onOpenChange={setIsOpen}>
    <PopoverTrigger>
      <Button variant='light'>
        <span className={`inline-block h-2.5 w-2.5 rounded-full ${status.dotColor}`} />
        <span>{ label }</span>
      </Button>
    </PopoverTrigger>
    <PopoverContent className='w-72 p-4'>
      <div className='w-full'>
        <p className='compact text-sm opacity-70'>{ t('status.setStatus') }</p>

        <div className='compact'>
          <Button
            className='w-full justify-start'
            variant='light'
            isDisabled={isBusy}
            onPress={() => applyStatus(ManualStatusKind.OUT_OF_OFFICE)}
          >
            <span className='inline-block h-2.5 w-2.5 rounded-full bg-secondary' />
            <span>{ t('status.outOfOffice') }</span>
          </Button>
          <Button
            className='w-full justify-start'
            variant='light'
            isDisabled={isBusy}
            onPress={() => applyStatus(ManualStatusKind.DO_NOT_DISTURB)}
          >
            <span className='inline-block h-2.5 w-2.5 rounded-full bg-danger' />
            <span>{ t('status.doNotDisturb') }</span>
          </Button>
        </div>

        <div className='compact'>
          <Input
            size='sm'
            label={t('status.customLabel')}
            value={customText}
            onChange={(event) => setCustomText(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter' && customText.trim()) {
                event.preventDefault()
                applyStatus(ManualStatusKind.CUSTOM, customText.trim())
              }
            }}
          />
        </div>

        <div className='compact'>
          <Select
            size='sm'
            label={t('status.durationLabel')}
            selectedKeys={[ durationKey ]}
            onChange={(event) => setDurationKey(event.target.value as DurationKey)}
          >
            <SelectItem key='hour1'>{ t('status.durations.hour1') }</SelectItem>
            <SelectItem key='hour4'>{ t('status.durations.hour4') }</SelectItem>
            <SelectItem key='day1'>{ t('status.durations.day1') }</SelectItem>
            <SelectItem key='week1'>{ t('status.durations.week1') }</SelectItem>
          </Select>
        </div>

        <div className='level'>
          { status.hasOverride
            ? <Button
              size='sm'
              variant='flat'
              isDisabled={isBusy}
              onPress={() => applyStatus(ManualStatusKind.UNSPECIFIED)}
            >
              <span>{ t('status.clearStatus') }</span>
            </Button>
            : <span /> }
          <Button
            size='sm'
            color='primary'
            isDisabled={isBusy || !customText.trim()}
            isLoading={isBusy}
            onPress={() => applyStatus(ManualStatusKind.CUSTOM, customText.trim())}
          >
            <span>{ t('status.customAction') }</span>
          </Button>
        </div>
      </div>
    </PopoverContent>
  </Popover>
}
