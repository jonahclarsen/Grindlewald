export function isScheduleEnabled(schedule, now = Date.now()) {
  return schedule.enabled && (schedule.disabledUntil == null || now >= schedule.disabledUntil);
}

export function disableScheduleFor(schedule, days, now = Date.now()) {
  if (!isScheduleEnabled(schedule, now)) throw new Error("Only enabled automations can be temporarily disabled");
  if (!Number.isInteger(days) || days < 1 || days > 8) throw new Error("Choose between 1 and 8 days");
  schedule.disabledUntil = now + days * 24 * 60 * 60 * 1000;
}

export function schedulePauseLabel(schedule, now = Date.now()) {
  if (!schedule.enabled) return "Disabled";
  if (isScheduleEnabled(schedule, now)) return "";
  return `Disabled until ${new Date(schedule.disabledUntil).toLocaleString([], {
    month: "short", day: "numeric", hour: "numeric", minute: "2-digit",
  })}`;
}
