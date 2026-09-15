// Copyright © 2026 Jalapeno Labs

import type { Satellite } from '../../../api/routes/satelliteRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { satelliteUpserted, selectAllSatellites } from '../../../store/satellitesSlice'

// User interface
import { Breadcrumbs, Button, Spinner, toast, useOverlayState } from '@heroui/react'
import { LuPlus } from 'react-icons/lu'
import { DeleteSatelliteDialog } from './DeleteSatelliteDialog'
import { SatelliteFormModal } from './SatelliteFormModal'
import { SatelliteTable } from './SatelliteTable'

// Misc
import { getUpstreamErrorMessage } from '../../../api/errors'
import { testSatellite, updateSatellite } from '../../../api/routes/satelliteRoutes'
import { useSatellitesLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'

export function ManageSatellitesPage() {
  const { t } = useTranslation([ 'satellites', 'settings', 'common' ])
  const dispatch = useAppDispatch()
  const satellites = useAppSelector(selectAllSatellites)
  const status = useSatellitesLoader()

  const formState = useOverlayState()
  const deleteState = useOverlayState()
  const [ selectedSatellite, setSelectedSatellite ] = useState<Satellite | null>(null)
  // Remounting the form per opening resets it to the chosen satellite's values.
  const [ formSession, setFormSession ] = useState(0)

  function openForm(satellite: Satellite | null) {
    setSelectedSatellite(satellite)
    setFormSession((session) => session + 1)
    formState.open()
  }

  async function toggleActive(satellite: Satellite) {
    try {
      const response = await updateSatellite(satellite.id, { isActive: !satellite.isActive })
      dispatch(satelliteUpserted(response.satellite))
      toast.success(t('toasts.updated', { name: satellite.name }))
    }
    catch (error) {
      console.debug('ManageSatellitesPage failed to toggle a satellite', { error, satelliteId: satellite.id })
      toast.danger(t('common:errors.unexpected'))
    }
  }

  async function runConnectionTest(satellite: Satellite) {
    try {
      const { result } = await testSatellite(satellite.id)
      toast.success(t('toasts.testPassed', {
        name: satellite.name,
        version: result.version,
        running: result.runningThreads,
        max: result.maxConcurrentThreads,
      }))
    }
    catch (error) {
      const message = getUpstreamErrorMessage(error)
      if (!message) {
        console.debug('ManageSatellitesPage failed to test a satellite', { error, satelliteId: satellite.id })
      }
      toast.danger(t('toasts.testFailed', { name: satellite.name }), {
        description: message ?? t('common:errors.unexpected'),
      })
    }
  }

  return <div className='container'>
    <Breadcrumbs className='compact'>
      <Breadcrumbs.Item href={UrlTree.settings}>{t('settings:title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item>{t('title')}</Breadcrumbs.Item>
    </Breadcrumbs>
    <h1 className='relaxed text-3xl font-bold'>{
      t('title')
    }</h1>

    <section>
      <div className='level compact items-start'>
        <div>
          <h2 className='text-xl font-semibold'>{
            t('fleet.heading')
          }</h2>
          <p className='mt-1 max-w-2xl text-sm opacity-70'>{
            t('fleet.description')
          }</p>
        </div>
        <Button
          size='sm'
          variant='outline'
          className='shrink-0'
          onPress={() => openForm(null)}
        >
          <LuPlus className='size-4' aria-hidden />
          <span>{t('fleet.add')}</span>
        </Button>
      </div>

      {status === 'loading' && <div className='grid place-items-center py-16'>
        <Spinner />
      </div>}

      {status === 'failed' && <p className='py-10 text-center text-sm text-danger'>{
        t('table.loadError')
      }</p>}

      {status === 'loaded' && <SatelliteTable
        satellites={satellites}
        onEdit={(satellite) => openForm(satellite)}
        onTest={runConnectionTest}
        onToggleActive={toggleActive}
        onDelete={(satellite) => {
          setSelectedSatellite(satellite)
          deleteState.open()
        }}
      />}
    </section>

    <SatelliteFormModal
      key={formSession}
      state={formState}
      satellite={selectedSatellite}
    />
    <DeleteSatelliteDialog
      state={deleteState}
      satellite={selectedSatellite}
    />
  </div>
}
