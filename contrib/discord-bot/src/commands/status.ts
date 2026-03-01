import {
  ChatInputCommandInteraction,
  SlashCommandBuilder,
  type SlashCommandOptionsOnlyBuilder,
} from 'discord.js';
import type { PulpodClient } from '../api/pulpod.js';
import { sessionEmbed, sessionListEmbed } from '../formatters/embed.js';

export const data: SlashCommandOptionsOnlyBuilder = new SlashCommandBuilder()
  .setName('status')
  .setDescription('Show session status')
  .addStringOption((opt) =>
    opt
      .setName('session')
      .setDescription('Session name or ID (omit for all sessions)')
      .setRequired(false)
      .setAutocomplete(true),
  );

export async function execute(
  interaction: ChatInputCommandInteraction,
  client: PulpodClient,
): Promise<void> {
  await interaction.deferReply();

  const sessionId = interaction.options.getString('session');

  try {
    if (sessionId) {
      const session = await client.getSession(sessionId);
      const embed = sessionEmbed(session);
      await interaction.editReply({ embeds: [embed] });
    } else {
      const sessions = await client.listSessions();
      const embed = sessionListEmbed(sessions);
      await interaction.editReply({ embeds: [embed] });
    }
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    await interaction.editReply(`Failed to get status: ${msg}`);
  }
}
