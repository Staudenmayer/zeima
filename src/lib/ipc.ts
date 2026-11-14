// Unwraps typed command results from src/bindings.ts. Errors become
// exceptions carrying the backend's code and field.

import { toast } from 'vue-sonner'
import type { CmdError } from '@/bindings'
import i18n from '@/plugins/i18n'

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

/** What the server says when a role lacks a permission: `not allowed: project.write`. */
const NOT_ALLOWED = /^not allowed: ([a-z_]+)\.([a-z_]+)$/

/** What the server says about roles and permissions (its words), in the person's language. */
const SERVER_WORDS: Record<string, string> = {
  'a role cannot have permissions you do not have yourself': 'errors.cannotGrant',
  'this role has permissions you do not have yourself': 'errors.cannotGiveRole',
  'this person has permissions you do not have': 'errors.holdsMore',
  'you cannot change the role you have yourself': 'errors.ownRole',
  'you cannot delete the role you have yourself': 'errors.ownRole',
  'the owner role cannot be changed': 'errors.ownerFixed',
  'the built-in roles cannot be deleted': 'errors.builtinRole',
  'only an owner can make owners': 'errors.onlyOwner',
  'a role with this name exists': 'errors.roleNameTaken',
}

export function errorMessage(err: unknown): string {
  if (err instanceof AppError && err.field) return `${err.field}: ${err.message}`
  if (err instanceof Error) {
    // Said in the person's language, with what they tried (the permission's name).
    const m = NOT_ALLOWED.exec(err.message)
    const key = m && `org.perm.${m[1]}.${m[2]}`
    if (key && i18n.global.te(key)) return i18n.global.t('errors.notAllowed', { what: i18n.global.t(key) })
    const known = SERVER_WORDS[err.message]
    if (known) return i18n.global.t(known)
    return err.message
  }
  return String(err)
}
