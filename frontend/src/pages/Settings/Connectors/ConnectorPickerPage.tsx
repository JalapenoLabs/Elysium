// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useSearchParams } from 'react-router'

// User interface
import { SearchField, Spinner } from '@heroui/react'
import { SettingsPage } from '../SettingsPage'
import { ConnectorKindCard } from './ConnectorKindCard'

// Misc
import { CONNECTOR_PROVIDES } from '../../../api/routes/connectorRoutes'
import { useFindHotkey } from '../../../hooks/useFindHotkey'
import { CONNECTOR_PROVIDES_PARAM, UrlTree } from '../../../urls'
import { searchKinds } from './connectorPresentation'
import { useConnectorKinds } from './useConnectorKinds'

// `/settings/connectors/new`: every kind as a searchable grid of logos and names, modeled on
// Onyx's. Each opens its own form page. `?provides=meetings` narrows it to the kinds that
// provide one kind of thing, so a page about meetings can offer every meeting source without
// naming one.
export function ConnectorPickerPage() {
  const { t } = useTranslation('connectors')
  const definitions = useConnectorKinds()
  const [ searchParams ] = useSearchParams()
  const [ search, setSearch ] = useState('')
  // Picking a kind starts by narrowing them, so typing starts there, and Ctrl+F comes back to it.
  const searchRef = useFindHotkey(true)

  const requested = searchParams.get(CONNECTOR_PROVIDES_PARAM)
  const provides = CONNECTOR_PROVIDES.find((candidate) => candidate === requested) ?? null
  if (requested && !provides) {
    console.debug('ConnectorPickerPage ignored an unknown provides filter', { requested })
  }

  const kinds = definitions.kinds
    .filter((definition) => !provides || definition.provides.includes(provides))
    .map((definition) => definition.kind)
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
      id='connector-kinds-search'
      autoComplete='off'
      className='relaxed'
      aria-label={t('picker.search')}
      value={search}
      onChange={setSearch}
    >
      <SearchField.Group>
        <SearchField.SearchIcon />
        <SearchField.Input ref={searchRef} placeholder={t('picker.search')} />
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
