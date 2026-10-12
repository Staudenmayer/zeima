/**
 * Where to attach a popover: an element's left and right edges at one
 * height (e.g. where it was clicked), following the element as it scrolls.
 * Attaching to a tall element as a whole would align the popover with its
 * top, which may be scrolled far out of view.
 */
export interface Anchor {
  getBoundingClientRect: () => DOMRect
  /** The element itself (scroll tracking, "inside" checks). */
  contextElement: HTMLElement
}

/**
 * Anchor on `el` at `clientY` (clamped into the element's visible part).
 * With `clientX`, the anchor is that exact point (like a context menu)
 * instead of the element's sides.
 */
export function pointAnchor(el: HTMLElement, clientY: number, clientX?: number): Anchor {
  const offset = clientY - el.getBoundingClientRect().top
  const offsetX = clientX === undefined ? undefined : clientX - el.getBoundingClientRect().left
  return {
    contextElement: el,
    getBoundingClientRect() {
      const r = el.getBoundingClientRect()
      // Keep the point inside the element's visible part (it may be clipped
      // by a scrolling parent).
      const clip = el.parentElement?.closest('.overflow-auto, .overflow-y-auto')?.getBoundingClientRect()
      const top = Math.max(r.top, clip?.top ?? -Infinity)
      const bottom = Math.min(r.bottom, clip?.bottom ?? Infinity)
      const y = Math.min(Math.max(r.top + offset, top), Math.max(top, bottom))
      if (offsetX !== undefined) return new DOMRect(r.left + offsetX, y, 0, 0)
      return new DOMRect(r.left, y, r.width, 0)
    },
  }
}
