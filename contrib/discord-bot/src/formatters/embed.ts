import { EmbedBuilder } from 'discord.js';
import type { InkConfig, Session, SessionEvent } from '../api/pulpod.js';

const STATUS_COLORS: Record<string, number> = {
  active: 0x2ecc71,
  ready: 0x3498db,
  stopped: 0xe74c3c,
  lost: 0xe67e22,
  idle: 0xf59e0b,
  creating: 0x95a5a6,
};

function statusColor(status: string): number {
  return STATUS_COLORS[status] ?? 0x95a5a6;
}

function statusEmoji(status: string): string {
  const emojis: Record<string, string> = {
    active: '\u{1F7E2}',
    ready: '\u{1F535}',
    stopped: '\u{1F534}',
    lost: '\u{1F7E0}',
    idle: '\u{1F7E1}',
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
      { name: 'ID', value: `\`${session.id}\``, inline: true },
    );

  if (session.ink) {
    embed.addFields({ name: 'Ink', value: session.ink, inline: true });
  }

  const command =
    session.command.length > 200 ? session.command.slice(0, 200) + '...' : session.command;
  embed.addFields({ name: 'Command', value: `\`${command}\``, inline: false });

  if (session.description) {
    const desc =
      session.description.length > 200
        ? session.description.slice(0, 200) + '...'
        : session.description;
    embed.addFields({ name: 'Description', value: desc, inline: false });
  }

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

export function inkListEmbed(inks: Record<string, InkConfig>): EmbedBuilder {
  const embed = new EmbedBuilder().setTitle('Inks').setColor(0x9b59b6);

  const entries = Object.entries(inks);
  if (entries.length === 0) {
    embed.setDescription('No inks configured.');
    return embed;
  }

  const lines = entries.map(([name, p]) => {
    const parts = [p.command, p.description].filter(Boolean);
    return `**${name}** — ${parts.join(': ') || 'default'}`;
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
    return `${statusEmoji(s.status)} **${s.name}** — ${s.status}`;
  });

  embed.setDescription(lines.join('\n'));
  if (sessions.length > 25) {
    embed.setFooter({ text: `Showing 25 of ${sessions.length} sessions` });
  }

  return embed;
}
