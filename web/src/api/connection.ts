import type { NodeInfo, PeerInfo } from './types';

function buildHeaders(token?: string): Record<string, string> {
  return token ? { Authorization: `Bearer ${token}` } : {};
}

export async function testConnection(url: string, token?: string): Promise<NodeInfo> {
  const headers = buildHeaders(token);
  const healthRes = await fetch(`${url}/api/v1/health`);
  if (!healthRes.ok) throw new Error('Health check failed');
  const nodeRes = await fetch(`${url}/api/v1/node`, { headers });
  if (!nodeRes.ok) throw new Error('Failed to fetch node info');
  return nodeRes.json();
}

export async function discoverPeers(url: string, token?: string): Promise<PeerInfo[]> {
  const headers = buildHeaders(token);
  const res = await fetch(`${url}/api/v1/peers`, { headers });
  if (!res.ok) throw new Error('Failed to discover peers');
  const data = await res.json();
  return data.peers;
}
