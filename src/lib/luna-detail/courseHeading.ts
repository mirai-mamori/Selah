export interface LunaCourseHeading {
  title: string;
  subtitle: string;
}

export function splitLunaCourseHeading(value: string): LunaCourseHeading {
  const normalized = String(value || "").replace(/\s+/g, " ").trim();
  const match = normalized.match(/^(.*?)\s+(\d{8})\s+(.+)$/);
  if (!match) return { title: normalized, subtitle: "" };

  const affiliation = match[1].trim();
  const courseCode = match[2];
  const title = match[3].trim();
  return {
    title: title || normalized,
    subtitle: [affiliation, courseCode].filter(Boolean).join(" · "),
  };
}
