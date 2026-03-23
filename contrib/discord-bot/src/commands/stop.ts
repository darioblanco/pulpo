import {
  ChatInputCommandInteraction,
  SlashCommandBuilder,
  type SlashCommandOptionsOnlyBuilder,
} from 'discord.js';
import type { PulpodClient } from '../api/pulpod.js';

export const data: SlashCommandOptionsOnlyBuilder = new SlashCommandBuilder()
  .setName('stop')
  .setDescription('Stop a running session')
  .addStringOption((opt) =>
    opt
      .setName('session')
      .setDescription('Session name or ID')
      .setRequired(true)
      .setAutocomplete(true),
  );

export async function execute(
  interaction: ChatInputCommandInteraction,
  client: PulpodClient,
): Promise<void> {
  await interaction.deferReply();

  const sessionId = interaction.options.getString('session', true);

  try {
    await client.stopSession(sessionId);
    await interaction.editReply(`Session \`${sessionId}\` stopped.`);
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    await interaction.editReply(`Failed to stop session: ${msg}`);
  }
}
