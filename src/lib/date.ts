function localTimestamp(
  year: number,
  month: number,
  day: number,
  hour = 0,
  minute = 0,
): number {
  const date = new Date(year, month - 1, day, hour, minute);
  if (
    date.getFullYear() !== year ||
    date.getMonth() !== month - 1 ||
    date.getDate() !== day
  ) {
    return 0;
  }
  return date.getTime();
}

export function parseNotificationTime(value: string, referenceDate = new Date()): number {
  const raw = value.trim();
  if (!raw) return 0;

  const iso = raw.match(/^\d{4}-\d{2}-\d{2}T/);
  if (iso) {
    const parsed = Date.parse(raw);
    if (Number.isFinite(parsed)) return parsed;
  }

  const normalized = raw
    .replace(/[年月.]/g, "/")
    .replace(/日/g, " ")
    .replace(/-/g, "/")
    .replace(/\s+/g, " ")
    .trim();

  const full = normalized.match(
    /(\d{4})\/(\d{1,2})\/(\d{1,2})(?:[ T](\d{1,2}):(\d{1,2}))?/,
  );
  if (full) {
    return localTimestamp(
      Number(full[1]),
      Number(full[2]),
      Number(full[3]),
      Number(full[4] ?? 0),
      Number(full[5] ?? 0),
    );
  }

  const short = normalized.match(
    /(?:^|[^\d])(\d{1,2})\/(\d{1,2})(?:[ T](\d{1,2}):(\d{1,2}))?/,
  );
  if (short) {
    return localTimestamp(
      referenceDate.getFullYear(),
      Number(short[1]),
      Number(short[2]),
      Number(short[3] ?? 0),
      Number(short[4] ?? 0),
    );
  }

  const fallback = Date.parse(raw.replace(" ", "T"));
  return Number.isFinite(fallback) ? fallback : 0;
}

export function compareNotificationDatesDesc(a: string, b: string): number {
  return parseNotificationTime(b) - parseNotificationTime(a);
}
