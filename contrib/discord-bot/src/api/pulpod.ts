import type { BotConfig } from '../config.js';

export interface Session {
  id: string;
  name: string;
  workdir: string;
  provider: string;
  prompt: string;
  status: string;
  mode: string;
  model?: string;
  ink?: string;
  metadata?: Record<string, string>;
  created_at: string;
  updated_at: string;
}

export interface CreateSessionRequest {
  name: string;
  workdir: string;
  provider?: string;
  prompt: string;
  mode?: string;
  ink?: string;
  model?: string;
  system_prompt?: string;
  metadata?: Record<string, string>;
  conversation_id?: string;
}

export interface InkConfig {
  description: string | null;
  provider?: string;
  model?: string;
  mode?: string;
  unrestricted?: boolean;
  instructions?: string;
  instructions_file?: string;
}

export interface SessionEvent {
  session_id: string;
  session_name: string;
  status: string;
  previous_status?: string;
  node_name: string;
  output_snippet?: string;
  timestamp: string;
}

export class PulpodClient {
  private baseUrl: string;
  private token: string;

  constructor(config: BotConfig) {
    this.baseUrl = config.pulpodUrl.replace(/\/+$/, '');
    this.token = config.pulpodToken;
  }

  private headers(): Record<string, string> {
    const h: Record<string, string> = { 'Content-Type': 'application/json' };
    if (this.token) {
      h['Authorization'] = `Bearer ${this.token}`;
    }
    return h;
  }

  async listSessions(): Promise<Session[]> {
    const res = await fetch(`${this.baseUrl}/api/v1/sessions`, {
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`Failed to list sessions: ${res.status}`);
    return res.json() as Promise<Session[]>;
  }

  async getSession(id: string): Promise<Session> {
    const res = await fetch(`${this.baseUrl}/api/v1/sessions/${encodeURIComponent(id)}`, {
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`Failed to get session: ${res.status}`);
    return res.json() as Promise<Session>;
  }

  async createSession(req: CreateSessionRequest): Promise<Session> {
    const res = await fetch(`${this.baseUrl}/api/v1/sessions`, {
      method: 'POST',
      headers: this.headers(),
      body: JSON.stringify(req),
    });
    if (!res.ok) {
      const body = await res.text();
      throw new Error(`Failed to create session (${res.status}): ${body}`);
    }
    return res.json() as Promise<Session>;
  }

  async killSession(id: string): Promise<void> {
    const res = await fetch(`${this.baseUrl}/api/v1/sessions/${encodeURIComponent(id)}/kill`, {
      method: 'POST',
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`Failed to kill session: ${res.status}`);
  }

  async deleteSession(id: string): Promise<void> {
    const res = await fetch(`${this.baseUrl}/api/v1/sessions/${encodeURIComponent(id)}`, {
      method: 'DELETE',
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`Failed to delete session: ${res.status}`);
  }

  async resumeSession(id: string): Promise<Session> {
    const res = await fetch(`${this.baseUrl}/api/v1/sessions/${encodeURIComponent(id)}/resume`, {
      method: 'POST',
      headers: this.headers(),
    });
    if (!res.ok) {
      const body = await res.text();
      throw new Error(`Failed to resume session (${res.status}): ${body}`);
    }
    return res.json() as Promise<Session>;
  }

  async sendInput(id: string, text: string): Promise<void> {
    const res = await fetch(`${this.baseUrl}/api/v1/sessions/${encodeURIComponent(id)}/input`, {
      method: 'POST',
      headers: this.headers(),
      body: JSON.stringify({ text }),
    });
    if (!res.ok) throw new Error(`Failed to send input: ${res.status}`);
  }

  async getOutput(id: string, lines?: number): Promise<string> {
    const params = lines ? `?lines=${lines}` : '';
    const res = await fetch(
      `${this.baseUrl}/api/v1/sessions/${encodeURIComponent(id)}/output${params}`,
      { headers: this.headers() },
    );
    if (!res.ok) throw new Error(`Failed to get output: ${res.status}`);
    return res.text();
  }

  async listInks(): Promise<{ inks: Record<string, InkConfig> }> {
    const res = await fetch(`${this.baseUrl}/api/v1/inks`, {
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`Failed to list inks: ${res.status}`);
    return res.json() as Promise<{ inks: Record<string, InkConfig> }>;
  }

  sseUrl(): string {
    const tokenParam = this.token ? `?token=${encodeURIComponent(this.token)}` : '';
    return `${this.baseUrl}/api/v1/events${tokenParam}`;
  }
}
