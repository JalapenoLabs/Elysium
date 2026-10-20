// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../store/hooks'
import { elysiaPanelToggled, selectElysia, selectIsElysiaPanelOpen } from '../store/elysiaSlice'

// User interface
import { Button, Tooltip } from '@heroui/react'

// Misc
import { useElysiaLoader } from '../hooks/useServerData'
import { lightDotClassNames, lightLabelKeys } from './presentation'

// Elysia in the topbar: her color at a glance, and the button that opens her panel beside
// the page. It is on every page, so it also keeps her status loaded for the sounds.
export function ElysiaToggle() {
  const { t } = useTranslation('elysia')
  const dispatch = useAppDispatch()
  useElysiaLoader()
  const elysia = useAppSelector(selectElysia)
  const isOpen = useAppSelector(selectIsElysiaPanelOpen)

  const light = elysia?.light ?? 'done'
  const status = t(lightLabelKeys[light])
  const label = isOpen
    ? t('toggle.close', { status })
    : t('toggle.open', { status })

  return <Tooltip delay={300}>
    <Button
      variant={isOpen
        ? 'secondary'
        : 'ghost'}
      aria-label={label}
      aria-pressed={isOpen}
      onPress={() => dispatch(elysiaPanelToggled())}
    >
      <span className={`block size-2.5 rounded-full ${lightDotClassNames[light]}`} aria-hidden />
      <span>{t('name')}</span>
    </Button>
    <Tooltip.Content>
      <span>{label}</span>
    </Tooltip.Content>
  </Tooltip>
}
