const BASE_URL = '/api/v1';

export async function getNode() {
  const res = await fetch(`${BASE_URL}/node`);
  return res.json();
}

export async function getSessions() {
  const res = await fetch(`${BASE_URL}/sessions`);
  return res.json();
}

export async function getSession(id: string) {
  const res = await fetch(`${BASE_URL}/sessions/${id}`);
  return res.json();
}

export async function createSession(data: {
  name?: string;
  repo_path: string;
  provider?: string;
  prompt: string;
}) {
  const res = await fetch(`${BASE_URL}/sessions`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data),
  });
  return res.json();
}

export async function killSession(id: string) {
  await fetch(`${BASE_URL}/sessions/${id}`, { method: 'DELETE' });
}
