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

// Keep original indices so sorting the display never changes an editor's target.
export function sortedScheduleEntries(schedules) {
  const dayMinute = ({ time }) => {
    const [hours, minutes] = time.split(":").map(Number);
    return (hours * 60 + minutes - 360 + 1440) % 1440;
  };
  return schedules.map((schedule, index) => ({ schedule, index }))
    .sort((a, b) => dayMinute(a.schedule) - dayMinute(b.schedule));
}

export function usesAllLights(schedule) {
  return schedule.allLights ?? schedule.lights.length === 0;
}
