// Graceful shutdown (SIGTERM / SIGINT), in the order a rolling update needs:
//   1. draining: /readyz answers 503 so the load balancer stops sending new work;
//   2. over the drain window, WebSockets are closed with 1012 at random moments (devices reconnect elsewhere, spread out);
//   3. HTTP is closed (app.close drains in-flight requests, then its onClose hooks run: the bus connection is unlistened and closed);
//   4. the reconcile timers are cleared and a running reconcile is waited for (bounded);
//   5. the database pools are ended.

import type { FastifyInstance } from 'fastify'

export interface ShutdownOptions {
  /** Milliseconds between the start of draining and closing the HTTP server. */
  drainMs: number
  /** Clears the reconcile timers. */
  stopTimers?: () => void
  /** The reconcile run in progress, if any. */
  inFlight?: () => Promise<unknown> | null
  /** Longest wait for that run. */
  inFlightLimitMs?: number
  /** Ends the database pools. */
  endPools?: () => Promise<unknown>
}

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms))

export async function gracefulShutdown(app: FastifyInstance, options: ShutdownOptions): Promise<void> {
  app.lifecycle.draining = true
  const { drainMs } = options
  if (drainMs > 0) {
    // Close the sockets within the first 80% of the window, leaving the rest for them to reconnect elsewhere.
    void app.closeSocketsSpread(drainMs * 0.8).catch(() => {})
    await sleep(drainMs)
  }
  await app.close()
  options.stopTimers?.()
  const running = options.inFlight?.()
  if (running) {
    await Promise.race([running.catch(() => {}), sleep(options.inFlightLimitMs ?? 5_000)])
  }
  await options.endPools?.()
}
