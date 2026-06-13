import { EmbedBuilder } from 'discord.js';
import type { PersonaConfig, Session, SessionEvent } from '../api/pulpod.js';
export declare function sessionEmbed(session: Session): EmbedBuilder;
export declare function eventEmbed(event: SessionEvent): EmbedBuilder;
export declare function personaListEmbed(personas: Record<string, PersonaConfig>): EmbedBuilder;
export declare function sessionListEmbed(sessions: Session[]): EmbedBuilder;
//# sourceMappingURL=embed.d.ts.map