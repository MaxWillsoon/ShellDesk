(function () {
  var storageKey = 'shelldesk:theme-preload';
  var light = {
    bg: '#f5f7fb',
    chrome: '#fbfcfe',
    surface: '#ffffff',
    text: '#172033',
  };
  var dark = {
    bg: '#0e131c',
    chrome: '#22272f',
    surface: '#111722',
    text: '#f4f7fb',
  };

  function readThemePreference() {
    try {
      var params = new URLSearchParams(window.location.search);
      var queryTheme = params.get('shelldeskTheme');

      if (queryTheme) {
        return queryTheme;
      }
    } catch {
      // Ignore URL parsing failures.
    }

    try {
      var storedTheme = window.localStorage.getItem(storageKey);

      if (!storedTheme) {
        return '';
      }

      if (storedTheme.charAt(0) === '{') {
        var parsedTheme = JSON.parse(storedTheme);
        return typeof parsedTheme.theme === 'string' ? parsedTheme.theme : '';
      }

      return storedTheme;
    } catch {
      return '';
    }
  }

  function getSystemTheme() {
    try {
      return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
    } catch {
      return 'light';
    }
  }

  function normalizeTheme(themePreference) {
    if (themePreference === 'dark' || themePreference === 'light') {
      return themePreference;
    }

    if (themePreference === 'system') {
      return getSystemTheme();
    }

    return 'dark';
  }

  function applyTheme(theme) {
    var palette = theme === 'light' ? light : dark;
    var root = document.documentElement;
    var colorSchemeMeta = document.querySelector('meta[name="color-scheme"]');

    root.setAttribute('data-theme', theme);
    root.style.colorScheme = theme;
    root.style.backgroundColor = palette.bg;
    root.style.setProperty('--bg', palette.bg);
    root.style.setProperty('--chrome', palette.chrome);
    root.style.setProperty('--surface', palette.surface);
    root.style.setProperty('--surface-elevated', theme === 'light' ? palette.chrome : palette.surface);
    root.style.setProperty('--text', palette.text);

    if (colorSchemeMeta) {
      colorSchemeMeta.setAttribute('content', theme);
    }
  }

  var theme = normalizeTheme(readThemePreference());
  applyTheme(theme);

  if (!document.body) {
    document.addEventListener('DOMContentLoaded', function () {
      applyTheme(theme);
    }, { once: true });
  }
}());
