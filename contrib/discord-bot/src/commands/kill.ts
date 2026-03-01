import {
  ChatInputCommandInteraction,
  SlashCommandBuilder,
  type SlashCommandOptionsOnlyBuilder,
} from 'discord.js';
import type { PulpodClient } from '../api/pulpod.js';

export const data: SlashCommandOptionsOnlyBuilder = new SlashCommandBuilder()
  .setName('kill')
  .setDescription('Kill a running session')
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
    await client.killSession(sessionId);
    await interaction.editReply(`Session \`${sessionId}\` killed.`);
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    await interaction.editReply(`Failed to kill session: ${msg}`);
  }
}
