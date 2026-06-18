import type { MessageId } from '../i18nCatalog';

export const defaultDesktopWallpaperPresetId = 'default';

export const desktopWallpaperPresets = [
  {
    id: defaultDesktopWallpaperPresetId,
    labelId: 'settings.wallpaper.preset.default',
  },
  {
    id: 'midnight-ops',
    labelId: 'settings.wallpaper.preset.midnightOps',
  },
  {
    id: 'amber-routes',
    labelId: 'settings.wallpaper.preset.amberRoutes',
  },
  {
    id: 'mist-console',
    labelId: 'settings.wallpaper.preset.mistConsole',
  },
  {
    id: 'green-health',
    labelId: 'settings.wallpaper.preset.greenHealth',
  },
  {
    id: 'indigo-traces',
    labelId: 'settings.wallpaper.preset.indigoTraces',
  },
] as const satisfies ReadonlyArray<{ id: string; labelId: MessageId }>;

export type DesktopWallpaperPreset = (typeof desktopWallpaperPresets)[number];
type DesktopWallpaperPresetId = DesktopWallpaperPreset['id'];

type WallpaperModule = { default: string };
type WallpaperLoader = () => Promise<WallpaperModule>;

const desktopWallpaperPresetLoaders: Record<DesktopWallpaperPresetId, WallpaperLoader> = {
  default: () => import('./images/default-desktop-wallpaper.png'),
  'midnight-ops': () => import('./images/desktop-wallpaper-midnight-ops.png'),
  'amber-routes': () => import('./images/desktop-wallpaper-amber-routes.png'),
  'mist-console': () => import('./images/desktop-wallpaper-mist-console.png'),
  'green-health': () => import('./images/desktop-wallpaper-green-health.png'),
  'indigo-traces': () => import('./images/desktop-wallpaper-indigo-traces.png'),
};

const loadedDesktopWallpaperUrls = new Map<DesktopWallpaperPresetId, string>();

export function getDesktopWallpaperPreset(presetId: string | null | undefined): DesktopWallpaperPreset {
  return desktopWallpaperPresets.find((preset) => preset.id === presetId) ?? desktopWallpaperPresets[0];
}

export function loadDesktopWallpaperPresetUrl(presetId: string | null | undefined) {
  const preset = getDesktopWallpaperPreset(presetId);
  const cachedUrl = loadedDesktopWallpaperUrls.get(preset.id);

  if (cachedUrl) {
    return Promise.resolve(cachedUrl);
  }

  return desktopWallpaperPresetLoaders[preset.id]().then((module) => {
    loadedDesktopWallpaperUrls.set(preset.id, module.default);
    return module.default;
  });
}
