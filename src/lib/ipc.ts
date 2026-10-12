// Unwraps typed command results from src/bindings.ts. Errors become
// exceptions carrying the backend's code and field.

import { toast } from 'vue-sonner'
import type { CmdError } from '@/bindings'

type Result<T> = { status: 'ok'; data: T } | { status: 'error'; error: CmdError }

export class AppError extends Error {
  readonly code: string
  readonly field: string | null

  constructor(error: CmdError) {
    super(error.message)
    this.code = error.code
    this.field = error.field
  }
}

export async function call<T>(promise: Promise<Result<T>>): Promise<T> {
  const result = await promise
  if (result.status === 'error') throw new AppError(result.error)
  return result.data
}

/** Like `call`, but shows a toast instead of throwing. Returns undefined on error. */
export async function callOrToast<T>(promise: Promise<Result<T>>): Promise<T | undefined> {
  try {
    return await call(promise)
  } catch (err) {
    toast.error(errorMessage(err))
    return undefined
  }
}

export function errorMessage(err: unknown): string {
  if (err instanceof AppError && err.field) return `${err.field}: ${err.message}`
  if (err instanceof Error) return err.message
  return String(err)
}
