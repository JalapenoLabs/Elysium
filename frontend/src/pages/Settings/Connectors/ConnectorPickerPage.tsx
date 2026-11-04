// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { SearchField, Spinner } from '@heroui/react'
import { SettingsPage } from '../SettingsPage'
import { ConnectorKindCard } from './ConnectorKindCard'

// Misc
import { UrlTree } from '../../../urls'
import { searchKinds } from './connectorPresentation'
import { useConnectorKinds } from './useConnectorKinds'

// `/settings/connectors/new`: every kind as a searchable grid of logos and names, modeled on
// Onyx's. Each opens its own form page.
export function ConnectorPickerPage() {
  const { t } = useTranslation('connectors')
  const definitions = useConnectorKinds()
  const [ search, setSearch ] = useState('')

  const kinds = definitions.kinds.map((definition) => definition.kind)
  const shown = searchKinds(
    kinds,
    search,
    (kind) => `${t(`kinds.${kind}.name`)} ${t(`kinds.${kind}.description`)}`,
  )

  return <SettingsPage
    title={t('picker.title')}
    parents={[{ label: t('page.title'), href: UrlTree.settingsConnectors }]}
  >
    <SearchField
      className='relaxed'
      aria-label={t('picker.search')}
      value={search}
      onChange={setSearch}
      autoFocus
    >
      <SearchField.Group>
        <SearchField.SearchIcon />
        <SearchField.Input placeholder={t('picker.search')} />
        <SearchField.ClearButton />
      </SearchField.Group>
    </SearchField>

    {definitions.isLoading && <div className='grid place-items-center py-10'><Spinner /></div>}
    {!definitions.isLoading && !shown.length && <p className='opacity-70'>{t('picker.noMatches')}</p>}
    <div className='grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4'>{
      shown.map((kind) => <ConnectorKindCard key={kind} kind={kind} />)
    }</div>
  </SettingsPage>
}
