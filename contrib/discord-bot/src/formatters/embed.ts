import { EmbedBuilder } from 'discord.js';
import type { PersonaConfig, Session, SessionEvent } from '../api/pulpod.js';

const STATUS_COLORS: Record<string, number> = {
  running: 0x2ecc71,
  completed: 0x3498db,
  dead: 0xe74c3c,
  stale: 0xe67e22,
  creating: 0x95a5a6,
};

function statusColor(status: string): number {
  return STATUS_COLORS[status] ?? 0x95a5a6;
}

function statusEmoji(status: string): string {
  const emojis: Record<string, string> = {
    running: '\u{1F7E2}',
    completed: '\u{1F535}',
    dead: '\u{1F534}',
    stale: '\u{1F7E0}',
    creating: '\u{26AA}',
  };
  return emojis[status] ?? '\u{26AA}';
}

export function sessionEmbed(session: Session): EmbedBuilder {
  const embed = new EmbedBuilder()
    .setTitle(`${statusEmoji(session.status)} ${session.name}`)
    .setColor(statusColor(session.status))
    .addFields(
      { name: 'Status', value: session.status, inline: true },
      { name: 'Provider', value: session.provider, inline: true },
      { name: 'ID', value: `\`${session.id}\``, inline: true },
    );

  if (session.model) {
    embed.addFields({ name: 'Model', value: session.model, inline: true });
  }
  if (session.persona) {
    embed.addFields({ name: 'Persona', value: session.persona, inline: true });
  }

  const prompt =
    session.prompt.length > 200 ? session.prompt.slice(0, 200) + '...' : session.prompt;
  embed.addFields({ name: 'Prompt', value: prompt, inline: false });
  embed.setTimestamp(new Date(session.created_at));

  return embed;
}

export function eventEmbed(event: SessionEvent): EmbedBuilder {
  const embed = new EmbedBuilder()
    .setTitle(`${statusEmoji(event.status)} Session: ${event.session_name}`)
    .setDescription(`Session \`${event.session_id}\` is now **${event.status}**`)
    .setColor(statusColor(event.status))
    .addFields(
      { name: 'Status', value: event.status, inline: true },
      { name: 'Node', value: event.node_name, inline: true },
    );

  if (event.previous_status) {
    embed.addFields({ name: 'Previous', value: event.previous_status, inline: true });
  }

  if (event.output_snippet) {
    const snippet =
      event.output_snippet.length > 1000
        ? event.output_snippet.slice(0, 1000) + '...'
        : event.output_snippet;
    embed.addFields({ name: 'Output', value: `\`\`\`\n${snippet}\n\`\`\``, inline: false });
  }

  embed.setTimestamp(new Date(event.timestamp));
  return embed;
}

export function personaListEmbed(personas: Record<string, PersonaConfig>): EmbedBuilder {
  const embed = new EmbedBuilder().setTitle('Personas').setColor(0x9b59b6);

  const entries = Object.entries(personas);
  if (entries.length === 0) {
    embed.setDescription('No personas configured.');
    return embed;
  }

  const lines = entries.map(([name, p]) => {
    const parts = [p.provider, p.model, p.mode, p.guard_preset].filter(Boolean);
    return `**${name}** — ${parts.join(', ') || 'default'}`;
  });

  embed.setDescription(lines.join('\n'));
  return embed;
}

export function sessionListEmbed(sessions: Session[]): EmbedBuilder {
  const embed = new EmbedBuilder().setTitle('Sessions').setColor(0x3498db);

  if (sessions.length === 0) {
    embed.setDescription('No sessions found.');
    return embed;
  }

  const lines = sessions.slice(0, 25).map((s) => {
    return `${statusEmoji(s.status)} **${s.name}** — ${s.status} (${s.provider})`;
  });

  embed.setDescription(lines.join('\n'));
  if (sessions.length > 25) {
    embed.setFooter({ text: `Showing 25 of ${sessions.length} sessions` });
  }

  return embed;
}
