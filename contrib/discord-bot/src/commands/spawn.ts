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
    opt.setName('workdir').setDescription('Working directory on the pulpod host').setRequired(true),
  )
  .addStringOption((opt) =>
    opt.setName('prompt').setDescription('Task prompt for the agent').setRequired(true),
  )
  .addStringOption((opt) =>
    opt.setName('persona').setDescription('Persona name (from pulpod config)').setRequired(false),
  )
  .addStringOption((opt) =>
    opt.setName('model').setDescription('Model override (e.g. opus, sonnet)').setRequired(false),
  )
  .addStringOption((opt) =>
    opt
      .setName('name')
      .setDescription('Session name (auto-generated if omitted)')
      .setRequired(false),
  );

export async function execute(
  interaction: ChatInputCommandInteraction,
  client: PulpodClient,
): Promise<void> {
  await interaction.deferReply();

  const workdir = interaction.options.getString('workdir', true);
  const prompt = interaction.options.getString('prompt', true);
  const persona = interaction.options.getString('persona') ?? undefined;
  const model = interaction.options.getString('model') ?? undefined;
  const name = interaction.options.getString('name') ?? undefined;

  try {
    const session = await client.createSession({
      workdir,
      prompt,
      persona,
      model,
      name,
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
