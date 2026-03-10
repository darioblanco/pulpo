function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error(`Failed to load sprite: ${src}`));
    img.src = src;
  });
}

export interface Sprites {
  octopus: Record<string, HTMLImageElement>;
  nodes: Record<string, HTMLImageElement>;
  ui: Record<string, HTMLImageElement>;
  status: Record<string, HTMLImageElement>;
  decor: Record<string, HTMLImageElement>;
}

const MANIFEST: Record<string, string[]> = {
  octopus: [
    'running-idle',
    'running-swim',
    'creating-idle',
    'creating-swim',
    'stale-idle',
    'stale-swim',
    'completed-idle',
    'completed-swim',
    'dead-idle',
    'dead-swim',
  ],
  nodes: ['coral-reef', 'sunken-ship', 'shipwreck'],
  ui: ['speech-bubble', 'card-border', 'icon-claude', 'icon-codex', 'icon-gemini', 'icon-opencode'],
  status: ['running', 'creating', 'stale', 'completed', 'dead'],
  decor: ['seaweed-1', 'seaweed-2', 'shell-1', 'shell-2', 'starfish'],
};

export async function loadAllSprites(): Promise<Sprites> {
  const sprites: Sprites = { octopus: {}, nodes: {}, ui: {}, status: {}, decor: {} };
  const promises: Promise<void>[] = [];

  for (const [category, names] of Object.entries(MANIFEST)) {
    for (const name of names) {
      promises.push(
        loadImage(`/sprites/${category}/${name}.png`)
          .then((img) => {
            sprites[category as keyof Sprites][name] = img;
          })
          .catch(() => {
            /* skip missing sprite */
          }),
      );
    }
  }

  await Promise.all(promises);
  return sprites;
}
