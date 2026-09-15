// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { CheckedDnsRecord, DnsRecordPurpose, DnsRecordStatus } from '../../../api/routes/mailRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Chip, Tooltip } from '@heroui/react'
import { LuCopy } from 'react-icons/lu'

const statusChipColors = {
  published: 'success',
  different: 'warning',
  missing: 'danger',
  unverified: 'default',
} as const satisfies Record<DnsRecordStatus, 'success' | 'warning' | 'danger' | 'default'>

const statusLabelKeys = {
  published: 'dns.statuses.published',
  different: 'dns.statuses.different',
  missing: 'dns.statuses.missing',
  unverified: 'dns.statuses.unverified',
} as const satisfies Record<DnsRecordStatus, ParseKeys<'email'>>

const purposeLabelKeys = {
  mx: 'dns.purposes.mx',
  spf: 'dns.purposes.spf',
  dkim: 'dns.purposes.dkim',
  dmarc: 'dns.purposes.dmarc',
} as const satisfies Record<DnsRecordPurpose, ParseKeys<'email'>>

type Props = {
  record: CheckedDnsRecord
  onCopy: (text: string) => void
}

// One recommended record and how public DNS compares. A status that needs explaining
// (a different value, a failed lookup) explains itself on hover.
export function DnsRecordRow(props: Props) {
  const { t } = useTranslation('email')
  const record = props.record

  const chip = <Chip size='sm' variant='soft' color={statusChipColors[record.status]}>{
    t(statusLabelKeys[record.status])
  }</Chip>
  const explanation = record.error ?? (record.status === 'different'
    ? t('dns.found', { values: record.found.join(' | ') })
    : null)

  return <tr className='border-t border-separator align-middle'>
    <td className='px-3 py-2 whitespace-nowrap'>{t(purposeLabelKeys[record.purpose])}</td>
    <td className='px-3 py-2 font-mono text-xs'>{record.recordType}</td>
    <td className='px-3 py-2'>
      <CopyableText text={record.name} label={t('dns.table.name')} onCopy={props.onCopy} />
    </td>
    <td className='px-3 py-2'>
      <CopyableText text={record.value} label={t('dns.table.value')} onCopy={props.onCopy} />
    </td>
    <td className='px-3 py-2 whitespace-nowrap'>{
      explanation
        ? <Tooltip delay={200}>
          <Tooltip.Trigger>{chip}</Tooltip.Trigger>
          <Tooltip.Content className='max-w-sm break-all'>
            <span>{explanation}</span>
          </Tooltip.Content>
        </Tooltip>
        : chip
    }</td>
  </tr>
}

type CopyableTextProps = {
  text: string
  label: string
  onCopy: (text: string) => void
}

function CopyableText(props: CopyableTextProps) {
  const { t } = useTranslation('email')

  return <div className='flex items-start gap-1'>
    <code className='min-w-0 flex-1 truncate font-mono text-xs leading-6' title={props.text}>{
      props.text
    }</code>
    <Button
      isIconOnly
      size='sm'
      variant='ghost'
      className='shrink-0'
      aria-label={t('dns.copy', { what: props.label })}
      onPress={() => props.onCopy(props.text)}
    >
      <LuCopy className='size-3.5' aria-hidden />
    </Button>
  </div>
}
