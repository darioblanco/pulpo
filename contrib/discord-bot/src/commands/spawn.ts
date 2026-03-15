import {
  ChatInputCommandInteraction,
  SlashCommandBuilder,
  type SlashCommandOptionsOnlyBuilder,
} from 'discord.js';
import type { PulpodClient } from '../api/pulpod.js';
import { sessionEmbed } from '../formatters/embed.js';

export const data: SlashCommandOptionsOnlyBuilder = new SlashCommandBuilder()
  .setName('spawn')
  .setDescription('Spawn a new agent session')
  .addStringOption((opt) =>
    opt.setName('name').setDescription('Session name').setRequired(true),
  )
  .addStringOption((opt) =>
    opt.setName('workdir').setDescription('Working directory on the pulpod host').setRequired(false),
  )
  .addStringOption((opt) =>
    opt.setName('command').setDescription('Shell command to run in the session').setRequired(false),
  )
  .addStringOption((opt) =>
    opt.setName('ink').setDescription('Ink name (from pulpod config)').setRequired(false),
  )
  .addStringOption((opt) =>
    opt
      .setName('description')
      .setDescription('Human-readable description of the session')
      .setRequired(false),
  );

export async function execute(
  interaction: ChatInputCommandInteraction,
  client: PulpodClient,
): Promise<void> {
  await interaction.deferReply();

  const name = interaction.options.getString('name', true);
  const workdir = interaction.options.getString('workdir') ?? undefined;
  const command = interaction.options.getString('command') ?? undefined;
  const ink = interaction.options.getString('ink') ?? undefined;
  const description = interaction.options.getString('description') ?? undefined;

  try {
    const session = await client.createSession({
      name,
      workdir,
      command,
      ink,
      description,
      metadata: {
        discord_channel_id: interaction.channelId,
        discord_user_id: interaction.user.id,
      },
    });

    const embed = sessionEmbed(session);
    await interaction.editReply({ embeds: [embed] });
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    await interaction.editReply(`Failed to spawn session: ${msg}`);
  }
}
