/** BCP 47 tags for Intl — drives decimal (`,` vs `.`) and grouping separators. */
const INTL_LOCALE: Record<string, string> = {
  en: "en-US",
  es: "es-ES",
  ja: "ja-JP",
  fr: "fr-FR",
  ko: "ko-KR",
  zh: "zh-CN",
  "zh-tw": "zh-TW",
  de: "de-DE",
  pt: "pt-BR",
  ru: "ru-RU",
  it: "it-IT",
};

export function intlLocale(appLocale: string): string {
  return INTL_LOCALE[appLocale] ?? "en-US";
}

export function formatNumber(
  value: number,
  appLocale: string,
  options?: Intl.NumberFormatOptions,
): string {
  if (!Number.isFinite(value)) return String(value);
  return new Intl.NumberFormat(intlLocale(appLocale), options).format(value);
}

export function formatInteger(value: number, appLocale: string): string {
  return formatNumber(value, appLocale, { maximumFractionDigits: 0 });
}

export function formatDecimal(
  value: number,
  appLocale: string,
  fractionDigits = 2,
): string {
  return formatNumber(value, appLocale, {
    minimumFractionDigits: fractionDigits,
    maximumFractionDigits: fractionDigits,
  });
}

/** Locale-aware decimal with optional trailing-zero trim (e.g. schedule pivots). */
export function formatDecimalTrimmed(
  value: number,
  appLocale: string,
  maxFractionDigits = 2,
): string {
  return formatNumber(value, appLocale, {
    minimumFractionDigits: 0,
    maximumFractionDigits: maxFractionDigits,
  });
}

export function formatPercent(
  value: number,
  appLocale: string,
  fractionDigits = 0,
): string {
  return `${formatNumber(value, appLocale, {
    minimumFractionDigits: fractionDigits,
    maximumFractionDigits: fractionDigits,
  })}%`;
}

export function formatBytes(bytes: number, appLocale: string): string {
  if (bytes < 1024) return `${formatInteger(bytes, appLocale)} B`;
  if (bytes < 1024 * 1024) return `${formatDecimal(bytes / 1024, appLocale, 0)} KB`;
  if (bytes < 1024 * 1024 * 1024) {
    return `${formatDecimal(bytes / (1024 * 1024), appLocale, 1)} MB`;
  }
  return `${formatDecimal(bytes / (1024 * 1024 * 1024), appLocale, 2)} GB`;
}

export function formatBytesPerSecond(bytesPerSec: number, appLocale: string): string {
  if (bytesPerSec <= 0) return "";
  if (bytesPerSec < 1024 * 1024) {
    return `${formatDecimal(bytesPerSec / 1024, appLocale, 0)} KB/s`;
  }
  return `${formatDecimal(bytesPerSec / (1024 * 1024), appLocale, 1)} MB/s`;
}

export function formatCompactCount(value: number, appLocale: string): string {
  if (value >= 1_000_000) {
    return `${formatDecimalTrimmed(value / 1_000_000, appLocale, 1)}M`;
  }
  if (value >= 1_000) {
    return `${formatDecimalTrimmed(value / 1_000, appLocale, 0)}k`;
  }
  return formatInteger(value, appLocale);
}

export function formatCompactCountUpperK(value: number, appLocale: string): string {
  if (value >= 1_000_000) {
    return `${formatDecimalTrimmed(value / 1_000_000, appLocale, 1)}M`;
  }
  if (value >= 1_000) {
    return `${formatDecimalTrimmed(value / 1_000, appLocale, 1)}K`;
  }
  return formatInteger(value, appLocale);
}

export function formatDateTime(
  value: Date | number | string,
  appLocale: string,
  options?: Intl.DateTimeFormatOptions,
): string {
  const date = value instanceof Date ? value : new Date(value);
  return date.toLocaleString(intlLocale(appLocale), options);
}

/** Parse typed decimals — accepts `,` or `.` as decimal separator. */
export function parseLocaleDecimal(raw: string): number {
  const trimmed = raw.trim();
  if (!trimmed) return NaN;
  const normalized = trimmed.replace(/\s/g, "").replace(",", ".");
  return parseFloat(normalized);
}

/** Display value for text inputs (uses locale decimal separator). */
export function formatDecimalForInput(
  value: number,
  appLocale: string,
  fractionDigits: number,
): string {
  return formatDecimal(value, appLocale, fractionDigits);
}
