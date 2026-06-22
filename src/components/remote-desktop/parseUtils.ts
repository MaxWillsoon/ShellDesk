const byteUnits = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'] as const;

interface FormatBytesOptions {
  invalidText?: string;
  zeroText?: string;
  maxUnit?: (typeof byteUnits)[number];
  fixedDecimal?: boolean;
}

export function formatBytes(bytes: number | null | undefined, options: FormatBytesOptions = {}): string {
  const invalidText = options.invalidText ?? '0 B';
  const zeroText = options.zeroText ?? invalidText;

  if (typeof bytes !== 'number' || !Number.isFinite(bytes) || bytes < 0) {
    return invalidText;
  }

  if (bytes === 0) {
    return zeroText;
  }

  const maxUnitIndex = Math.max(0, byteUnits.indexOf(options.maxUnit ?? 'PB'));
  let value = bytes;
  let unitIndex = 0;

  while (value >= 1024 && unitIndex < maxUnitIndex) {
    value /= 1024;
    unitIndex += 1;
  }

  if (options.fixedDecimal) {
    return `${value.toFixed(unitIndex === 0 ? 0 : 1)} ${byteUnits[unitIndex]}`;
  }

  const precision = value >= 100 ? 0 : value >= 10 ? 1 : 2;
  return `${value.toFixed(precision).replace(/\.0+$/, '')} ${byteUnits[unitIndex]}`;
}

export function toStringOrEmpty(value: unknown): string {
  if (typeof value === 'string') {
    return value.trim();
  }

  if (typeof value === 'number' || typeof value === 'boolean') {
    return String(value);
  }

  return '';
}

export function readString(record: Record<string, unknown> | undefined, ...keys: string[]): string {
  if (!record) {
    return '';
  }

  for (const key of keys) {
    const value = record[key];

    if (typeof value === 'string') {
      return value.trim();
    }

    if (typeof value === 'number' || typeof value === 'boolean') {
      return String(value);
    }

    if (Array.isArray(value)) {
      return value
        .filter((item) => item !== undefined && item !== null)
        .map((item) => String(item).trim())
        .filter(Boolean)
        .join(', ');
    }
  }

  return '';
}

export function readInteger(value: unknown): number | undefined {
  if (typeof value === 'number' && Number.isInteger(value)) {
    return value;
  }

  if (typeof value !== 'string' || !value.trim()) {
    return undefined;
  }

  const parsedValue = Number.parseInt(value, 10);
  return Number.isInteger(parsedValue) ? parsedValue : undefined;
}

export function readNumber(record: Record<string, unknown>, ...keys: string[]): number | undefined;
export function readNumber(record: Record<string, unknown> | undefined, ...keys: string[]): number | undefined;
export function readNumber(value: string | number | undefined | null): number | undefined;
export function readNumber(
  recordOrValue: Record<string, unknown> | string | number | undefined | null,
  ...keys: string[]
): number | undefined {
  if (!keys.length || !recordOrValue || typeof recordOrValue !== 'object' || Array.isArray(recordOrValue)) {
    if (typeof recordOrValue === 'number' && Number.isFinite(recordOrValue)) {
      return recordOrValue;
    }

    if (typeof recordOrValue !== 'string' || !recordOrValue.trim() || recordOrValue === '-') {
      return undefined;
    }

    const parsedValue = Number.parseFloat(recordOrValue);
    return Number.isFinite(parsedValue) ? parsedValue : undefined;
  }

  for (const key of keys) {
    const value = recordOrValue[key];

    if (typeof value === 'number' && Number.isFinite(value)) {
      return value;
    }

    if (typeof value === 'string') {
      const parsedValue = Number(value);

      if (Number.isFinite(parsedValue)) {
        return parsedValue;
      }
    }
  }

  return undefined;
}

export function clampPercent(value: number | undefined): number {
  if (value === undefined) {
    return 0;
  }

  return Math.min(Math.max(value, 0), 100);
}
