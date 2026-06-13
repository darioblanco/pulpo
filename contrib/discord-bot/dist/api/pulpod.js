export class PulpodClient {
    baseUrl;
    token;
    constructor(config) {
        this.baseUrl = config.pulpodUrl.replace(/\/+$/, '');
        this.token = config.pulpodToken;
    }
    headers() {
        const h = { 'Content-Type': 'application/json' };
        if (this.token) {
            h['Authorization'] = `Bearer ${this.token}`;
        }
        return h;
    }
    async listSessions() {
        const res = await fetch(`${this.baseUrl}/api/v1/sessions`, {
            headers: this.headers(),
        });
        if (!res.ok)
            throw new Error(`Failed to list sessions: ${res.status}`);
        return res.json();
    }
    async getSession(id) {
        const res = await fetch(`${this.baseUrl}/api/v1/sessions/${encodeURIComponent(id)}`, {
            headers: this.headers(),
        });
        if (!res.ok)
            throw new Error(`Failed to get session: ${res.status}`);
        return res.json();
    }
    async createSession(req) {
        const res = await fetch(`${this.baseUrl}/api/v1/sessions`, {
            method: 'POST',
            headers: this.headers(),
            body: JSON.stringify(req),
        });
        if (!res.ok) {
            const body = await res.text();
            throw new Error(`Failed to create session (${res.status}): ${body}`);
        }
        return res.json();
    }
    async killSession(id) {
        const res = await fetch(`${this.baseUrl}/api/v1/sessions/${encodeURIComponent(id)}`, {
            method: 'DELETE',
            headers: this.headers(),
        });
        if (!res.ok)
            throw new Error(`Failed to kill session: ${res.status}`);
    }
    async resumeSession(id) {
        const res = await fetch(`${this.baseUrl}/api/v1/sessions/${encodeURIComponent(id)}/resume`, {
            method: 'POST',
            headers: this.headers(),
        });
        if (!res.ok) {
            const body = await res.text();
            throw new Error(`Failed to resume session (${res.status}): ${body}`);
        }
        return res.json();
    }
    async sendInput(id, text) {
        const res = await fetch(`${this.baseUrl}/api/v1/sessions/${encodeURIComponent(id)}/input`, {
            method: 'POST',
            headers: this.headers(),
            body: JSON.stringify({ text }),
        });
        if (!res.ok)
            throw new Error(`Failed to send input: ${res.status}`);
    }
    async getOutput(id, lines) {
        const params = lines ? `?lines=${lines}` : '';
        const res = await fetch(`${this.baseUrl}/api/v1/sessions/${encodeURIComponent(id)}/output${params}`, { headers: this.headers() });
        if (!res.ok)
            throw new Error(`Failed to get output: ${res.status}`);
        return res.text();
    }
    async listPersonas() {
        const res = await fetch(`${this.baseUrl}/api/v1/personas`, {
            headers: this.headers(),
        });
        if (!res.ok)
            throw new Error(`Failed to list personas: ${res.status}`);
        return res.json();
    }
    sseUrl() {
        const tokenParam = this.token ? `?token=${encodeURIComponent(this.token)}` : '';
        return `${this.baseUrl}/api/v1/events${tokenParam}`;
    }
}
//# sourceMappingURL=pulpod.js.map