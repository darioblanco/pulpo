import {
  ChatInputCommandInteraction,
  SlashCommandBuilder,
  type SlashCommandOptionsOnlyBuilder,
} from 'discord.js';
import type { PulpodClient } from '../api/pulpod.js';
import { inkListEmbed } from '../formatters/embed.js';

export const data: SlashCommandOptionsOnlyBuilder = new SlashCommandBuilder()
  .setName('inks')
  .setDescription('List available ink configurations');

export async function execute(
  interaction: ChatInputCommandInteraction,
  client: PulpodClient,
): Promise<void> {
  await interaction.deferReply();

  try {
    const { inks } = await client.listInks();
    const embed = inkListEmbed(inks);
    await interaction.editReply({ embeds: [embed] });
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    await interaction.editReply(`Failed to list inks: ${msg}`);
  }
}
