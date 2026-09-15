// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { Llm } from '../../../api/routes/llmRoutes'

// Core
import { generatePath, useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { llmDeleted, llmUpserted, selectAllLlms } from '../../../store/llmsSlice'

// User interface
import { Breadcrumbs, Button, Dropdown, Label, Spinner, toast } from '@heroui/react'
import { LuChevronDown, LuPlus } from 'react-icons/lu'
import { LlmTable } from './LlmTable'

// Misc
import { deleteLlm, updateLlm } from '../../../api/routes/llmRoutes'
import { useConfirm } from '../../../hooks/useConfirm'
import { useLlmsLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'
import { LLM_TYPES, llmTypeLabelKeys } from './llmPresentation'
import { addLlmUrlByType } from './llmProviders'

export function ManageLlmsPage() {
  const { t } = useTranslation([ 'llms', 'settings', 'common' ])
  const dispatch = useAppDispatch()
  const llms = useAppSelector(selectAllLlms)
  const status = useLlmsLoader()

  const navigate = useNavigate()
  const confirm = useConfirm()

  function confirmDelete(llm: Llm) {
    confirm({
      title: t('delete.title', { name: llm.name }),
      message: t('delete.body'),
      tone: 'danger',
      confirmText: t('common:actions.delete'),
      onConfirm: async () => {
        try {
          await deleteLlm(llm.id)
        }
        catch (error) {
          console.debug('ManageLlmsPage failed to delete the LLM', { error, llmId: llm.id })
          toast.danger(t('common:errors.unexpected'))
          throw error
        }
        dispatch(llmDeleted(llm.id))
        toast.success(t('toasts.deleted', { name: llm.name }))
      },
    })
  }

  function openEditor(llm: Llm) {
    navigate(generatePath(UrlTree.settingsLlmsEdit, { llmId: llm.id }))
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
        <Dropdown>
          <Button
            size='sm'
            variant='outline'
            className='shrink-0'
          >
            <LuPlus className='size-4' aria-hidden />
            <span>{t('credentials.add')}</span>
            <LuChevronDown className='size-4' aria-hidden />
          </Button>
          <Dropdown.Popover placement='bottom end'>
            <Dropdown.Menu
              aria-label={t('credentials.addMenu')}
              onAction={(key: Key) => navigate(addLlmUrlByType[key as Llm['type']])}
            >{
              LLM_TYPES.map((type) => <Dropdown.Item key={type} id={type} textValue={t(llmTypeLabelKeys[type])}>
                <Label>{t(llmTypeLabelKeys[type])}</Label>
              </Dropdown.Item>)
            }</Dropdown.Menu>
          </Dropdown.Popover>
        </Dropdown>
      </div>

      {status === 'loading' && <div className='grid place-items-center py-16'>
        <Spinner />
      </div>}

      {status === 'failed' && <p className='py-10 text-center text-sm text-danger'>{
        t('table.loadError')
      }</p>}

      {status === 'loaded' && <LlmTable
        llms={llms}
        onEdit={openEditor}
        onToggleActive={toggleActive}
        onDelete={confirmDelete}
      />}
    </section>
  </div>
}
