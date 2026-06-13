import { EventSource } from 'eventsource';
import type { Client } from 'discord.js';
import type { PulpodClient } from '../api/pulpod.js';
import type { BotConfig } from '../config.js';
export declare function startSseListener(discordClient: Client, pulpodClient: PulpodClient, config: BotConfig): EventSource;
//# sourceMappingURL=sse.d.ts.map