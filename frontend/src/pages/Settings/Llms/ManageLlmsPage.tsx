// Copyright © 2026 Jalapeno Labs

import type { Llm } from '../../../api/routes/llmRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { llmUpserted, selectAllLlms } from '../../../store/llmsSlice'

// User interface
import { Breadcrumbs, Button, Spinner, toast, useOverlayState } from '@heroui/react'
import { LuPlus } from 'react-icons/lu'
import { DeleteLlmDialog } from './DeleteLlmDialog'
import { LlmFormModal } from './LlmFormModal'
import { LlmTable } from './LlmTable'

// Misc
import { updateLlm } from '../../../api/routes/llmRoutes'
import { useLlmsLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'

export function ManageLlmsPage() {
  const { t } = useTranslation([ 'llms', 'settings', 'common' ])
  const dispatch = useAppDispatch()
  const llms = useAppSelector(selectAllLlms)
  const status = useLlmsLoader()

  const formState = useOverlayState()
  const deleteState = useOverlayState()
  const [ selectedLlm, setSelectedLlm ] = useState<Llm | null>(null)
  // Remounting the form per opening resets it to the chosen credential's values.
  const [ formSession, setFormSession ] = useState(0)

  function openForm(llm: Llm | null) {
    setSelectedLlm(llm)
    setFormSession((session) => session + 1)
    formState.open()
  }

  async function toggleActive(llm: Llm) {
    try {
      const response = await updateLlm(llm.id, { isActive: !llm.isActive })
      dispatch(llmUpserted(response.llm))
      toast.success(t('toasts.updated', { name: llm.name }))
    }
    catch (error) {
      console.debug('ManageLlmsPage failed to toggle an LLM', { error, llmId: llm.id })
      toast.danger(t('common:errors.unexpected'))
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
            t('credentials.heading')
          }</h2>
          <p className='mt-1 max-w-2xl text-sm opacity-70'>{
            t('credentials.description')
          }</p>
        </div>
        <Button
          size='sm'
          variant='outline'
          className='shrink-0'
          onPress={() => openForm(null)}
        >
          <LuPlus className='size-4' aria-hidden />
          <span>{t('credentials.add')}</span>
        </Button>
      </div>

      {status === 'loading' && <div className='grid place-items-center py-16'>
        <Spinner />
      </div>}

      {status === 'failed' && <p className='py-10 text-center text-sm text-danger'>{
        t('table.loadError')
      }</p>}

      {status === 'loaded' && <LlmTable
        llms={llms}
        onEdit={(llm) => openForm(llm)}
        onToggleActive={toggleActive}
        onDelete={(llm) => {
          setSelectedLlm(llm)
          deleteState.open()
        }}
      />}
    </section>

    <LlmFormModal
      key={formSession}
      state={formState}
      llm={selectedLlm}
    />
    <DeleteLlmDialog
      state={deleteState}
      llm={selectedLlm}
    />
  </div>
}
