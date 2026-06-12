import type { BotConfig } from '../config.js';
export interface Session {
    id: string;
    name: string;
    repo_path: string;
    provider: string;
    prompt: string;
    status: string;
    mode: string;
    model?: string;
    persona?: string;
    metadata?: Record<string, string>;
    created_at: string;
    updated_at: string;
}
export interface CreateSessionRequest {
    name?: string;
    repo_path: string;
    provider?: string;
    prompt: string;
    mode?: string;
    persona?: string;
    model?: string;
    system_prompt?: string;
    metadata?: Record<string, string>;
}
export interface PersonaConfig {
    provider?: string;
    model?: string;
    mode?: string;
    guard_preset?: string;
    allowed_tools?: string[];
    system_prompt?: string;
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
export declare class PulpodClient {
    private baseUrl;
    private token;
    constructor(config: BotConfig);
    private headers;
    listSessions(): Promise<Session[]>;
    getSession(id: string): Promise<Session>;
    createSession(req: CreateSessionRequest): Promise<Session>;
    killSession(id: string): Promise<void>;
    resumeSession(id: string): Promise<Session>;
    sendInput(id: string, text: string): Promise<void>;
    getOutput(id: string, lines?: number): Promise<string>;
    listPersonas(): Promise<{
        personas: Record<string, PersonaConfig>;
    }>;
    sseUrl(): string;
}
//# sourceMappingURL=pulpod.d.ts.map