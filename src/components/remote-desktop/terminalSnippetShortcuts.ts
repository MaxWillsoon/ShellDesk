export interface ShortcutKeyEventLike {
  key: string;
  code: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

const physicalShortcutKeyNames: Record<string, string> = {
  Backquote: '`',
  Minus: '-',
  Equal: '=',
  BracketLeft: '[',
  BracketRight: ']',
  Backslash: '\\',
  Semicolon: ';',
  Quote: "'",
  Comma: ',',
  Period: '.',
  Slash: '/',
};

export function isMacClient() {
  if (typeof navigator === 'undefined') {
    return false;
  }

  return /mac|iphone|ipad|ipod/i.test(navigator.platform);
}

function physicalShortcutKeyName(event: ShortcutKeyEventLike) {
  if (/^Key[A-Z]$/u.test(event.code)) {
    return event.code.slice(3);
  }

  if (/^Digit[0-9]$/u.test(event.code)) {
    return event.code.slice(5);
  }

  if (/^F(?:[1-9]|1[0-9]|2[0-4])$/u.test(event.code)) {
    return event.code;
  }

  return physicalShortcutKeyNames[event.code] ?? null;
}

function shortcutEventKey(event: ShortcutKeyEventLike) {
  return physicalShortcutKeyName(event) ?? event.key;
}

function normalizeShortcutKeyName(rawKey: string) {
  if (rawKey === ' ') {
    return 'Space';
  }

  if (rawKey === 'ArrowUp') {
    return '↑';
  }

  if (rawKey === 'ArrowDown') {
    return '↓';
  }

  if (rawKey === 'ArrowLeft') {
    return '←';
  }

  if (rawKey === 'ArrowRight') {
    return '→';
  }

  if (rawKey === 'Escape') {
    return 'Esc';
  }

  if (rawKey === 'Backspace') {
    return '⌫';
  }

  if (rawKey === 'Delete') {
    return 'Del';
  }

  if (rawKey === 'Enter') {
    return '↵';
  }

  if (rawKey === 'Tab') {
    return 'Tab';
  }

  if (/^[a-z]$/u.test(rawKey)) {
    return rawKey.toUpperCase();
  }

  return rawKey;
}

export function keyEventToShortcut(event: ShortcutKeyEventLike, macClient: boolean) {
  if (['Meta', 'Control', 'Alt', 'Shift'].includes(event.key)) {
    return '';
  }

  const parts: string[] = [];

  if (macClient) {
    if (event.metaKey) {
      parts.push('⌘');
    }
    if (event.ctrlKey) {
      parts.push('⌃');
    }
    if (event.altKey) {
      parts.push('⌥');
    }
    if (event.shiftKey) {
      parts.push('Shift');
    }
  } else {
    if (event.ctrlKey) {
      parts.push('Ctrl');
    }
    if (event.altKey) {
      parts.push('Alt');
    }
    if (event.shiftKey) {
      parts.push('Shift');
    }
    if (event.metaKey) {
      parts.push('Win');
    }
  }

  parts.push(normalizeShortcutKeyName(shortcutEventKey(event)));
  return parts.join(' + ');
}

function parseShortcut(shortcut: string) {
  const parts = shortcut
    .split('+')
    .map((part) => part.trim())
    .filter(Boolean);

  if (!parts.length) {
    return null;
  }

  const key = parts.pop() ?? '';
  return { modifiers: parts, key };
}

export function matchesSnippetShortcut(event: ShortcutKeyEventLike, shortcut: string, macClient: boolean) {
  const parsedShortcut = parseShortcut(shortcut);

  if (!parsedShortcut || ['Meta', 'Control', 'Alt', 'Shift'].includes(event.key)) {
    return false;
  }

  const { modifiers, key } = parsedShortcut;
  const hasMacModifier = modifiers.some((modifier) => modifier === '⌘' || modifier === '⌃' || modifier === '⌥');
  const hasPcModifier = modifiers.some((modifier) => modifier === 'Ctrl' || modifier === 'Alt' || modifier === 'Win');

  if ((!macClient && hasMacModifier) || (macClient && hasPcModifier)) {
    return false;
  }

  if (macClient) {
    if (event.metaKey !== modifiers.includes('⌘')) {
      return false;
    }
    if (event.ctrlKey !== modifiers.includes('⌃')) {
      return false;
    }
    if (event.altKey !== modifiers.includes('⌥')) {
      return false;
    }
    if (event.shiftKey !== modifiers.includes('Shift')) {
      return false;
    }
  } else {
    if (event.ctrlKey !== modifiers.includes('Ctrl')) {
      return false;
    }
    if (event.altKey !== modifiers.includes('Alt')) {
      return false;
    }
    if (event.shiftKey !== modifiers.includes('Shift')) {
      return false;
    }
    if (event.metaKey !== modifiers.includes('Win')) {
      return false;
    }
  }

  return normalizeShortcutKeyName(shortcutEventKey(event)).toLowerCase() === normalizeShortcutKeyName(key).toLowerCase();
}
