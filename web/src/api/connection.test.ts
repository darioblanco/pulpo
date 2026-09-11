import { describe, it, expect, vi, beforeEach } from 'vitest';
import { testConnection } from './connection';

const mockFetch = vi.fn();
vi.stubGlobal('fetch', mockFetch);

beforeEach(() => {
  mockFetch.mockReset();
});

describe('testConnection', () => {
  it('returns node info on success', async () => {
    const nodeInfo = { name: 'mac-mini', hostname: 'mac-mini.local' };
    mockFetch
      .mockResolvedValueOnce({ ok: true })
      .mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(nodeInfo) });

    const result = await testConnection('http://mac-mini:7433');

    expect(mockFetch).toHaveBeenCalledWith('http://mac-mini:7433/api/v1/health');
    expect(mockFetch).toHaveBeenCalledWith('http://mac-mini:7433/api/v1/node', { headers: {} });
    expect(result).toEqual(nodeInfo);
  });

  it('throws on health check failure', async () => {
    mockFetch.mockResolvedValueOnce({ ok: false });

    await expect(testConnection('http://bad:7433')).rejects.toThrow('Health check failed');
  });

  it('throws on node info failure', async () => {
    mockFetch.mockResolvedValueOnce({ ok: true }).mockResolvedValueOnce({ ok: false });

    await expect(testConnection('http://bad:7433')).rejects.toThrow('Failed to fetch node info');
  });

  it('sends auth header when token is provided', async () => {
    const nodeInfo = { name: 'mac-mini' };
    mockFetch
      .mockResolvedValueOnce({ ok: true })
      .mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(nodeInfo) });

    await testConnection('http://mac-mini:7433', 'my-token');

    expect(mockFetch).toHaveBeenCalledWith('http://mac-mini:7433/api/v1/node', {
      headers: { Authorization: 'Bearer my-token' },
    });
  });
});
