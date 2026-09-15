// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'
import type { MailDomain } from '../../../api/routes/mailRoutes'

// Core
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { Button, Modal, Spinner, toast } from '@heroui/react'
import { LuCopy, LuRefreshCw } from 'react-icons/lu'
import { DnsRecordRow } from './DnsRecordRow'

// Misc
import { getUpstreamErrorMessage } from '../../../api/errors'
import { checkMailDomainDns } from '../../../api/routes/mailRoutes'

type Props = {
  state: UseOverlayStateReturn
  domain: MailDomain | null
  // The mail server's hostname, which the internet requirements name.
  hostname: string
}

// The records a domain needs, and what public DNS serves for each. Checked every time the
// modal opens and on demand, since records change at the operator's DNS provider.
export function MailDomainDnsModal(props: Props) {
  const { t } = useTranslation([ 'email', 'common' ])

  // Only this modal asks, so it stays in SWR. No key while closed: nothing is looked up.
  const dnsKey = props.state.isOpen && props.domain
    ? `v1/mail/domains/${props.domain.id}/dns`
    : null
  const domainId = props.domain?.id ?? ''
  const { data, error, isValidating, mutate } = useSWR(
    dnsKey,
    () => checkMailDomainDns(domainId),
    { revalidateOnFocus: false },
  )

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text)
      toast.success(t('toasts.copied'))
    }
    catch (copyError) {
      console.debug('MailDomainDnsModal could not copy to the clipboard', { copyError })
      toast.danger(t('toasts.copyFailed'))
    }
  }

  const hasDkim = data?.records.some((record) => record.purpose === 'dkim')

  return <Modal.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-4xl'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{
            t('dns.title', { name: props.domain?.name ?? '' })
          }</Modal.Heading>
          <p className='mt-1 text-sm opacity-70'>{
            t('dns.description')
          }</p>
        </Modal.Header>
        <Modal.Body className='mt-2 flex flex-col gap-4'>
          {!data && !error && <div className='grid place-items-center py-12'>
            <Spinner />
          </div>}

          {error && <p className='py-8 text-center text-sm text-danger'>{
            getUpstreamErrorMessage(error) ?? t('dns.loadError')
          }</p>}

          {data && <>
            <div className='overflow-x-auto rounded-xl border border-separator'>
              {/* Fixed layout: names and DKIM keys are long, and truncate within their column. */}
              <table className='w-full min-w-[720px] table-fixed text-left text-sm' aria-label={t('dns.table.label')}>
                <colgroup>
                  <col className='w-24' />
                  <col className='w-14' />
                  <col className='w-[34%]' />
                  <col />
                  <col className='w-36' />
                </colgroup>
                <thead className='text-xs opacity-70'>
                  <tr>
                    <th className='px-3 py-2 font-medium'>{t('dns.table.purpose')}</th>
                    <th className='px-3 py-2 font-medium'>{t('dns.table.type')}</th>
                    <th className='px-3 py-2 font-medium'>{t('dns.table.name')}</th>
                    <th className='px-3 py-2 font-medium'>{t('dns.table.value')}</th>
                    <th className='px-3 py-2 font-medium'>{t('dns.table.status')}</th>
                  </tr>
                </thead>
                <tbody>{
                  data.records.map((record) => <DnsRecordRow
                    key={`${record.name}-${record.value}`}
                    record={record}
                    onCopy={copy}
                  />)
                }</tbody>
              </table>
            </div>

            {!hasDkim && <p className='text-xs opacity-70'>{
              t('dns.dkimPending')
            }</p>}

            <details className='text-sm'>
              <summary className='cursor-pointer'>{t('dns.zoneFile')}</summary>
              <p className='mt-1 text-xs opacity-70'>{t('dns.zoneFileHint')}</p>
              <div className='relative mt-2'>
                <pre className='max-h-60 overflow-auto rounded-md bg-default p-3 pr-12 text-xs'>{
                  data.zoneFile
                }</pre>
                <Button
                  isIconOnly
                  size='sm'
                  variant='ghost'
                  className='absolute top-2 right-2'
                  aria-label={t('dns.copy', { what: t('dns.zoneFile') })}
                  onPress={() => copy(data.zoneFile)}
                >
                  <LuCopy className='size-4' aria-hidden />
                </Button>
              </div>
            </details>

            <div className='rounded-xl bg-default p-4 text-sm'>
              <p className='font-medium'>{t('dns.internet.title')}</p>
              <p className='mt-1 opacity-80'>{
                t('dns.internet.description', { hostname: props.hostname })
              }</p>
            </div>
          </>}
        </Modal.Body>
        <Modal.Footer>
          <Button
            variant='tertiary'
            isPending={isValidating}
            isDisabled={!data}
            onPress={() => mutate()}
          >
            <LuRefreshCw className='size-4' aria-hidden />
            <span>{t('dns.recheck')}</span>
          </Button>
          <Button slot='close'>
            <span>{t('common:actions.close')}</span>
          </Button>
        </Modal.Footer>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}
