function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error(`Failed to load sprite: ${src}`));
    img.src = src;
  });
}

export type BackgroundSprites = Record<string, HTMLImageElement>;

export interface Sprites {
  octopus: Record<string, HTMLImageElement>;
  nodes: Record<string, HTMLImageElement>;
  ui: Record<string, HTMLImageElement>;
  status: Record<string, HTMLImageElement>;
  decor: Record<string, HTMLImageElement>;
  fauna: Record<string, HTMLImageElement>;
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
  decor: ['seaweed-1', 'seaweed-2', 'shell-1', 'shell-2', 'starfish', 'kelp'],
  fauna: [
    'angelfish',
    'clownfish',
    'fish-gold',
    'silverfish',
    'tang',
    'jellyfish',
    'turtle',
    'bubbles',
    'shark-1',
    'shark-2',
    'shark-3',
    'shark-4',
    'shark-5',
    'shark-6',
    'shark-7',
    'shark-8',
    'shark-9',
  ],
};

const BACKGROUND_LAYERS = ['sea-background'];

export async function loadAllSprites(): Promise<Sprites> {
  const sprites: Sprites = { octopus: {}, nodes: {}, ui: {}, status: {}, decor: {}, fauna: {} };
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

export async function loadBackground(): Promise<BackgroundSprites> {
  const bg: BackgroundSprites = {};
  const promises: Promise<void>[] = [];

  for (const name of BACKGROUND_LAYERS) {
    promises.push(
      loadImage(`/sprites/background/${name}.png`)
        .then((img) => {
          bg[name] = img;
        })
        .catch(() => {
          /* skip missing layer */
        }),
    );
  }

  await Promise.all(promises);
  return bg;
}
