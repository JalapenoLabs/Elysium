// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Radio, RadioGroup } from '@heroui/react'

// Misc
import { useThemePreference } from '../../../hooks/useThemePreference'
import { THEME_PREFERENCES } from '../../../theme/themePreference'
import { themeOptions } from './themeOptions'

// Theme cards built on HeroUI's RadioGroup for keyboard and screen reader behavior,
// using uikit's preview artwork. uikit's own ThemeSelector hardcodes its brand green
// for the selected card, which would clash with this app's accent.
export function ThemeSelector() {
  const { t } = useTranslation('settings')
  const { preference, setPreference } = useThemePreference()

  return <RadioGroup
    aria-label={t('personalDetails.appearance.label')}
    orientation='horizontal'
    value={preference}
    onChange={(value) => {
      const match = THEME_PREFERENCES.find((option) => option === value)
      if (!match) {
        console.debug('ThemeSelector ignored an unknown theme value', { value })
        return
      }
      setPreference(match)
    }}
    className='grid max-w-xl grid-cols-3 gap-3'
  >{
    themeOptions.map((option) => <Radio
      key={option.preference}
      value={option.preference}
      className='m-0'
    >
      <Radio.Content
        className={[
          'flex w-full cursor-pointer flex-col gap-2 rounded-xl border-2 border-separator p-3 transition-colors',
          'data-[hovered=true]:border-accent/40',
          'data-[selected=true]:border-accent data-[selected=true]:ring-4 data-[selected=true]:ring-accent/15',
          'data-[focus-visible=true]:ring-4 data-[focus-visible=true]:ring-accent/30',
        ].join(' ')}
      >
        <option.Preview className='block h-auto w-full rounded-md' aria-hidden />
        <div className='flex items-center gap-2'>
          <Radio.Control className='border border-separator'>
            <Radio.Indicator />
          </Radio.Control>
          <span className='text-sm font-semibold'>{
            t(option.labelKey)
          }</span>
        </div>
      </Radio.Content>
    </Radio>)
  }</RadioGroup>
}
