import {
  ChatInputCommandInteraction,
  SlashCommandBuilder,
  type SlashCommandOptionsOnlyBuilder,
} from 'discord.js';
import type { PulpodClient } from '../api/pulpod.js';

export const data: SlashCommandOptionsOnlyBuilder = new SlashCommandBuilder()
  .setName('logs')
  .setDescription('Show recent session output')
  .addStringOption((opt) =>
    opt
      .setName('session')
      .setDescription('Session name or ID')
      .setRequired(true)
      .setAutocomplete(true),
  )
  .addIntegerOption((opt) =>
    opt
      .setName('lines')
      .setDescription('Number of lines to show (default: 50)')
      .setRequired(false)
      .setMinValue(1)
      .setMaxValue(500),
  );

export async function execute(
  interaction: ChatInputCommandInteraction,
  client: PulpodClient,
): Promise<void> {
  await interaction.deferReply();

  const sessionId = interaction.options.getString('session', true);
  const lines = interaction.options.getInteger('lines') ?? 50;

  try {
    const output = await client.getOutput(sessionId, lines);

    if (!output.trim()) {
      await interaction.editReply('No output available.');
      return;
    }

    // Discord message limit is 2000 chars. Truncate if needed.
    const maxLen = 1900;
    const truncated =
      output.length > maxLen ? '...' + output.slice(output.length - maxLen) : output;

    await interaction.editReply(`\`\`\`\n${truncated}\n\`\`\``);
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    await interaction.editReply(`Failed to get logs: ${msg}`);
  }
}
