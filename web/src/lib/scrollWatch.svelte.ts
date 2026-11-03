/** How far down a page counts as worth a way back to the top. */
const FAR_PX = 600
/**
 * How far it has to move against the current direction to count as turning.
 * Small movements come from a settling finger, or the page itself making room.
 */
const TURN_PX = 24
/** How long a jump's own scrolling is ignored. */
const QUIET_MS = 600

/**
 * Which way the page is being scrolled: going back up is when someone wants the
 * list's options again, or the top. For use while a component is being set up.
 */
export function watchScroll() {
  let up = $state(false)
  let far = $state(false)
  /** The furthest point in the direction it's going, to measure a turn from. */
  let furthest = 0
  let quietUntil = 0

  $effect(() => {
    furthest = scrollY
    const moved = () => {
      const now = scrollY
      far = now > FAR_PX
      if (performance.now() < quietUntil) {
        furthest = now
      } else if (now <= 0) {
        // At the very top they're back in their usual place anyway.
        up = false
        furthest = 0
      } else if (up) {
        furthest = Math.min(furthest, now)
        if (now > furthest + TURN_PX) {
          up = false
          furthest = now
        }
      } else {
        furthest = Math.max(furthest, now)
        if (now < furthest - TURN_PX) {
          up = true
          furthest = now
        }
      }
    }
    addEventListener('scroll', moved, { passive: true })
    return () => removeEventListener('scroll', moved)
  })

  return {
    get up() {
      return up
    },
    get far() {
      return far
    },
    /** The page is about to be moved on purpose, so put the options away. */
    quiet() {
      up = false
      quietUntil = performance.now() + QUIET_MS
    },
  }
}
