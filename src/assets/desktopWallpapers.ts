import defaultDesktopWallpaperUrl from './images/default-desktop-wallpaper.png';
import amberRoutesWallpaperUrl from './images/desktop-wallpaper-amber-routes.png';
import greenHealthWallpaperUrl from './images/desktop-wallpaper-green-health.png';
import indigoTracesWallpaperUrl from './images/desktop-wallpaper-indigo-traces.png';
import midnightOpsWallpaperUrl from './images/desktop-wallpaper-midnight-ops.png';
import mistConsoleWallpaperUrl from './images/desktop-wallpaper-mist-console.png';

export const defaultDesktopWallpaperPresetId = 'default';

export const desktopWallpaperPresets = [
  {
    id: defaultDesktopWallpaperPresetId,
    label: '默认背景',
    url: defaultDesktopWallpaperUrl,
  },
  {
    id: 'midnight-ops',
    label: '深蓝运维',
    url: midnightOpsWallpaperUrl,
  },
  {
    id: 'amber-routes',
    label: '暖色链路',
    url: amberRoutesWallpaperUrl,
  },
  {
    id: 'mist-console',
    label: '浅色控制台',
    url: mistConsoleWallpaperUrl,
  },
  {
    id: 'green-health',
    label: '绿色健康',
    url: greenHealthWallpaperUrl,
  },
  {
    id: 'indigo-traces',
    label: '蓝紫观测',
    url: indigoTracesWallpaperUrl,
  },
] as const;

export type DesktopWallpaperPreset = (typeof desktopWallpaperPresets)[number];

export function getDesktopWallpaperPreset(presetId: string | null | undefined): DesktopWallpaperPreset {
  return desktopWallpaperPresets.find((preset) => preset.id === presetId) ?? desktopWallpaperPresets[0];
}
