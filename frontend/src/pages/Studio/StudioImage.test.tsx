// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'

// User interface
import { StudioImage } from './StudioImage'

const FALLBACK_TEXT = 'No image yet'

function renderImage(src: string, onLoadStateChange = vi.fn()) {
  return render(<StudioImage
    src={src}
    alt='Banana'
    fallback={<span>{FALLBACK_TEXT}</span>}
    onLoadStateChange={onLoadStateChange}
  />)
}

describe('StudioImage', () => {
  it('shows the image and reports it loaded', () => {
    const onLoadStateChange = vi.fn()
    renderImage('/one.png', onLoadStateChange)

    fireEvent.load(screen.getByRole('img', { name: 'Banana' }))

    expect(onLoadStateChange).toHaveBeenLastCalledWith('loaded')
    expect(screen.queryByText(FALLBACK_TEXT)).toBeNull()
  })

  it('replaces an image that fails to load with the fallback and reports it', () => {
    const onLoadStateChange = vi.fn()
    renderImage('/one.png', onLoadStateChange)

    fireEvent.error(screen.getByRole('img', { name: 'Banana' }))

    expect(onLoadStateChange).toHaveBeenLastCalledWith('failed')
    expect(screen.queryByRole('img', { name: 'Banana' })).toBeNull()
    expect(screen.getByText(FALLBACK_TEXT)).toBeTruthy()
  })

  it('tries a new file afresh after an earlier one failed', () => {
    const view = renderImage('/one.png')
    fireEvent.error(screen.getByRole('img', { name: 'Banana' }))

    view.rerender(<StudioImage
      src='/two.png'
      alt='Banana'
      fallback={<span>{FALLBACK_TEXT}</span>}
    />)

    expect(screen.getByRole('img', { name: 'Banana' }).getAttribute('src')).toBe('/two.png')
    expect(screen.queryByText(FALLBACK_TEXT)).toBeNull()
  })
})
