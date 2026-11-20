import { beforeEach, describe, expect, it, vi } from 'vitest'

const orgRequest = vi.fn()
vi.mock('@/bindings', () => ({ commands: { orgRequest: (...args: unknown[]) => orgRequest(...args) } }))
vi.mock('@/plugins/i18n', () => ({ default: { global: { t: (k: string) => k, te: () => false } } }))
vi.mock('vue-sonner', () => ({ toast: { error: vi.fn() } }))

const { ok, getV1Me, getV1OrgsByOrgIdMembers, getV1OrgsByOrgIdAudit, postV1Orgs, deleteV1OrgsByOrgIdJobsByJobId } = await import('./api')

const answer = (data: unknown) => orgRequest.mockResolvedValueOnce({ status: 'ok', data })

beforeEach(() => orgRequest.mockReset())

describe('the typed API client', () => {
  it('sends a GET with its path parameters and returns the JSON', async () => {
    answer(JSON.stringify([{ id: 'u1', email: null, name: 'Ada', role: 'member', active: true, lastSeenAt: null }]))
    const members = ok(await getV1OrgsByOrgIdMembers({ path: { orgId: 'o1' } }))
    expect(orgRequest).toHaveBeenCalledWith('GET', '/v1/orgs/o1/members', null)
    expect(members[0]?.name).toBe('Ada')
  })

  it('adds the query to the path', async () => {
    answer('[]')
    await getV1OrgsByOrgIdAudit({ path: { orgId: 'o1' }, query: { limit: 50, before: 7 } })
    expect(orgRequest).toHaveBeenCalledWith('GET', '/v1/orgs/o1/audit?limit=50&before=7', null)
  })

  it('sends the body of a POST as JSON text', async () => {
    answer(JSON.stringify({ id: 'o2' }))
    const created = ok(await postV1Orgs({ body: { name: 'Acme' } }))
    expect(orgRequest).toHaveBeenCalledWith('POST', '/v1/orgs', '{"name":"Acme"}')
    expect(created.id).toBe('o2')
  })

  it('returns nothing for an answer without content', async () => {
    answer(null)
    const result = await deleteV1OrgsByOrgIdJobsByJobId({ path: { orgId: 'o1', jobId: 'j1' } })
    expect(orgRequest).toHaveBeenCalledWith('DELETE', '/v1/orgs/o1/jobs/j1', null)
    expect(ok(result)).toBeUndefined()
  })

  it('throws what the backend answers with when the call fails', async () => {
    orgRequest.mockResolvedValueOnce({ status: 'error', error: { code: 'signed_out', message: 'sign in', field: null } })
    await expect(getV1Me()).rejects.toMatchObject({ code: 'signed_out' })
  })
})
