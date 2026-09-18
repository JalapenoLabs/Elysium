// Copyright © 2026 Jalapeno Labs

import type { KeyboardEvent, PointerEvent } from 'react'
import type { BurnupPoint } from '../../api/routes/initiativeRoutes'
import type { ChartBox } from './burnup'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Misc
import { useElementWidth } from '../../hooks/useElementWidth'
import { createBurnupScales, getNearestPointIndex, getStepPath } from './burnup'

// Room for the count ticks on the left, the direct end labels on the right, and the dates
// underneath.
const CHART_HEIGHT = 240
const PADDING = { top: 12, right: 96, bottom: 28, left: 36 } as const
const END_LABEL_GAP = 10
// The two end labels are held at least this far apart, so equal counts stay readable.
const END_LABEL_MIN_SPACING = 16

type Props = {
  points: BurnupPoint[]
}

// The burnup: total scope and resolved items over time, both as steps, so added scope rises
// as scope instead of pulling a percentage down. A crosshair snaps to the nearest change and
// shows both counts, by pointer or by arrow keys, and the same numbers are listed in a table
// below the chart.
export function BurnupChart(props: Props) {
  const { t, i18n } = useTranslation('initiatives')
  const { ref, width } = useElementWidth<HTMLDivElement>()
  const [ activeIndex, setActiveIndex ] = useState<number | null>(null)
  const points = props.points
  const dateFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium' })
  const dateTimeFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium', timeStyle: 'short' })

  const box: ChartBox = { width, height: CHART_HEIGHT, padding: PADDING }
  const scales = createBurnupScales(points, box)
  const last = points[points.length - 1]
  const active = activeIndex === null
    ? null
    : points[activeIndex]

  function onPointerMove(event: PointerEvent<SVGSVGElement>) {
    const bounds = event.currentTarget.getBoundingClientRect()
    setActiveIndex(getNearestPointIndex(points, scales.timeAt(event.clientX - bounds.left)))
  }

  function onKeyDown(event: KeyboardEvent<SVGSVGElement>) {
    const steps = {
      ArrowLeft: -1,
      ArrowRight: 1,
    } as const
    if (!(event.key === 'ArrowLeft' || event.key === 'ArrowRight')) {
      return
    }
    event.preventDefault()
    const current = activeIndex ?? points.length - 1
    setActiveIndex(Math.min(Math.max(current + steps[event.key], 0), points.length - 1))
  }

  // Keeps the end labels from sitting on top of each other when the counts are close.
  const totalLabelY = scales.y(last.total)
  const resolvedLabelY = Math.max(scales.y(last.resolved), totalLabelY + END_LABEL_MIN_SPACING)
  const plotRight = width - PADDING.right
  const plotBottom = CHART_HEIGHT - PADDING.bottom

  return <figure>
    <figcaption className='compact flex flex-wrap items-center gap-4 text-sm'>
      <span className='inline-flex items-center gap-2'>
        <span className='inline-block h-0.5 w-4 rounded-full bg-chart-scope' aria-hidden />
        <span>{t('burnup.total')}</span>
      </span>
      <span className='inline-flex items-center gap-2'>
        <span className='inline-block h-0.5 w-4 rounded-full bg-chart-resolved' aria-hidden />
        <span>{t('burnup.resolved')}</span>
      </span>
    </figcaption>

    <div ref={ref} className='relative'>
      {width > 0 && <svg
        width={width}
        height={CHART_HEIGHT}
        role='img'
        aria-label={t('burnup.label', { resolved: last.resolved, total: last.total })}
        tabIndex={0}
        className={[
          'block overflow-visible rounded-md',
          'focus-visible:ring-2 focus-visible:ring-focus focus-visible:outline-none',
        ].join(' ')}
        onPointerMove={onPointerMove}
        onPointerLeave={() => setActiveIndex(null)}
        onKeyDown={onKeyDown}
        onBlur={() => setActiveIndex(null)}
      >
        {/* Count gridlines and ticks: hairlines, one step off the page */}
        {scales.ticks.map((tick) => <g key={tick}>
          <line
            x1={PADDING.left}
            x2={plotRight}
            y1={scales.y(tick)}
            y2={scales.y(tick)}
            className='stroke-separator'
            strokeWidth={1}
          />
          <text
            x={PADDING.left - 8}
            y={scales.y(tick)}
            textAnchor='end'
            dominantBaseline='middle'
            className='fill-muted text-xs tabular-nums'
          >{tick}</text>
        </g>)}

        {/* The first and last moments along the bottom */}
        <text x={PADDING.left} y={plotBottom + 18} className='fill-muted text-xs'>{
          dateFormatter.format(new Date(points[0].at))
        }</text>
        <text x={plotRight} y={plotBottom + 18} textAnchor='end' className='fill-muted text-xs'>{
          t('burnup.now')
        }</text>

        {/* Resolved gets a faint wash beneath it; scope stays a line */}
        <path
          d={`${getStepPath(points, scales, 'resolved')} V${plotBottom} H${PADDING.left} Z`}
          className='fill-chart-resolved'
          fillOpacity={0.1}
        />
        <path
          d={getStepPath(points, scales, 'total')}
          fill='none'
          className='stroke-chart-scope'
          strokeWidth={2}
          strokeLinejoin='round'
          strokeLinecap='round'
        />
        <path
          d={getStepPath(points, scales, 'resolved')}
          fill='none'
          className='stroke-chart-resolved'
          strokeWidth={2}
          strokeLinejoin='round'
          strokeLinecap='round'
        />

        {/* End markers with a ring in the page color, and direct labels */}
        <circle
          cx={plotRight}
          cy={scales.y(last.total)}
          r={4}
          className='fill-chart-scope stroke-background'
          strokeWidth={2}
        />
        <circle
          cx={plotRight}
          cy={scales.y(last.resolved)}
          r={4}
          className='fill-chart-resolved stroke-background'
          strokeWidth={2}
        />
        <text
          x={plotRight + END_LABEL_GAP}
          y={totalLabelY}
          dominantBaseline='middle'
          className='fill-foreground text-xs'
        >{
          t('burnup.totalEnd', { count: last.total })
        }</text>
        <text
          x={plotRight + END_LABEL_GAP}
          y={resolvedLabelY}
          dominantBaseline='middle'
          className='fill-foreground text-xs'
        >{
          t('burnup.resolvedEnd', { count: last.resolved })
        }</text>

        {/* The crosshair, at the change nearest the pointer */}
        {active && <line
          x1={scales.x(Date.parse(active.at))}
          x2={scales.x(Date.parse(active.at))}
          y1={PADDING.top}
          y2={plotBottom}
          className='stroke-muted'
          strokeWidth={1}
        />}
      </svg>}

      {active && <div
        className='pointer-events-none absolute top-0 rounded-lg bg-overlay px-3 py-2 text-xs shadow-md'
        style={{
          left: Math.min(scales.x(Date.parse(active.at)) + 12, Math.max(width - 180, 0)),
        }}
        role='status'
      >
        <p className='mb-1 opacity-70'>{dateTimeFormatter.format(new Date(active.at))}</p>
        <p className='flex items-center gap-2'>
          <span className='inline-block h-0.5 w-3 rounded-full bg-chart-scope' aria-hidden />
          <span className='font-semibold tabular-nums'>{active.total}</span>
          <span className='opacity-70'>{t('burnup.total')}</span>
        </p>
        <p className='flex items-center gap-2'>
          <span className='inline-block h-0.5 w-3 rounded-full bg-chart-resolved' aria-hidden />
          <span className='font-semibold tabular-nums'>{active.resolved}</span>
          <span className='opacity-70'>{t('burnup.resolved')}</span>
        </p>
      </div>}
    </div>

    <details className='mt-2 text-sm'>
      <summary className='cursor-pointer opacity-70'>{t('burnup.showTable')}</summary>
      <div className='mt-2 max-h-64 overflow-auto'>
        <table className='w-full text-left'>
          <thead>
            <tr className='opacity-70'>
              <th className='py-1 font-medium'>{t('burnup.when')}</th>
              <th className='py-1 text-right font-medium'>{t('burnup.total')}</th>
              <th className='py-1 text-right font-medium'>{t('burnup.resolved')}</th>
            </tr>
          </thead>
          <tbody>{
            points.map((point, index) => <tr key={index} className='border-t border-separator'>
              <td className='py-1'>{dateTimeFormatter.format(new Date(point.at))}</td>
              <td className='py-1 text-right tabular-nums'>{point.total}</td>
              <td className='py-1 text-right tabular-nums'>{point.resolved}</td>
            </tr>)
          }</tbody>
        </table>
      </div>
    </details>
  </figure>
}
