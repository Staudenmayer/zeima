/** Where a segment sits among the segments it overlaps. */
export interface Lanes {
  start: number
  end: number
  lane: number
  lanes: number
}

/**
 * Greedy lanes within clusters of mutually overlapping segments: segments
 * that overlap share their cluster's space side by side, the others get all
 * of it.
 */
export function layoutLanes<T extends { start: number; end: number }>(segments: T[]): (T & Lanes)[] {
  const sorted = segments.filter((s) => s.end > s.start).sort((a, b) => a.start - b.start || b.end - a.end)
  const blocks: (T & Lanes)[] = []
  let cluster: (T & Lanes)[] = []
  let laneEnds: number[] = []
  let clusterEnd = -Infinity
  const closeCluster = () => {
    for (const b of cluster) b.lanes = laneEnds.length
    cluster = []
    laneEnds = []
  }
  for (const s of sorted) {
    if (s.start >= clusterEnd) closeCluster()
    let lane = laneEnds.findIndex((end) => end <= s.start)
    if (lane === -1) lane = laneEnds.push(s.end) - 1
    else laneEnds[lane] = s.end
    const block = { ...s, lane, lanes: 1 }
    cluster.push(block)
    blocks.push(block)
    clusterEnd = Math.max(clusterEnd, s.end)
  }
  closeCluster()
  return blocks
}
