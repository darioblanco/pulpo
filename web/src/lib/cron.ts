const DAYS_OF_WEEK = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];

interface CronFields {
  minute: string;
  hour: string;
  dayOfMonth: string;
  month: string;
  dayOfWeek: string;
}

function parseCron(expr: string): CronFields | null {
  const parts = expr.trim().split(/\s+/);
  if (parts.length !== 5) return null;
  return {
    minute: parts[0],
    hour: parts[1],
    dayOfMonth: parts[2],
    month: parts[3],
    dayOfWeek: parts[4],
  };
}

function formatTime(hour: number, minute: number): string {
  const h = hour % 12 || 12;
  const ampm = hour < 12 ? 'AM' : 'PM';
  const m = minute.toString().padStart(2, '0');
  return `${h}:${m} ${ampm}`;
}

function isNumeric(s: string): boolean {
  return /^\d+$/.test(s);
}

function isStep(s: string): boolean {
  return /^\*\/\d+$/.test(s);
}

function stepValue(s: string): number {
  return parseInt(s.split('/')[1], 10);
}

/**
 * Returns a human-readable description of a cron expression.
 * Handles common patterns; falls back to the raw expression for complex ones.
 */
export function describeCron(expr: string): string {
  const fields = parseCron(expr);
  if (!fields) return expr;

  const { minute, hour, dayOfMonth, month, dayOfWeek } = fields;

  // Every minute: * * * * *
  if (minute === '*' && hour === '*' && dayOfMonth === '*' && month === '*' && dayOfWeek === '*') {
    return 'Every minute';
  }

  // Every N minutes: */N * * * *
  if (isStep(minute) && hour === '*' && dayOfMonth === '*' && month === '*' && dayOfWeek === '*') {
    const n = stepValue(minute);
    return n === 1 ? 'Every minute' : `Every ${n} minutes`;
  }

  // Every hour at :MM: MM * * * *
  if (
    isNumeric(minute) &&
    hour === '*' &&
    dayOfMonth === '*' &&
    month === '*' &&
    dayOfWeek === '*'
  ) {
    const m = parseInt(minute, 10);
    return m === 0 ? 'Every hour' : `Every hour at :${m.toString().padStart(2, '0')}`;
  }

  // Every N hours: 0 */N * * *
  if (
    isNumeric(minute) &&
    isStep(hour) &&
    dayOfMonth === '*' &&
    month === '*' &&
    dayOfWeek === '*'
  ) {
    const n = stepValue(hour);
    return n === 1 ? 'Every hour' : `Every ${n} hours`;
  }

  // Daily at HH:MM: MM HH * * *
  if (
    isNumeric(minute) &&
    isNumeric(hour) &&
    dayOfMonth === '*' &&
    month === '*' &&
    dayOfWeek === '*'
  ) {
    return `Every day at ${formatTime(parseInt(hour, 10), parseInt(minute, 10))}`;
  }

  // Weekly on DOW at HH:MM: MM HH * * DOW
  if (
    isNumeric(minute) &&
    isNumeric(hour) &&
    dayOfMonth === '*' &&
    month === '*' &&
    isNumeric(dayOfWeek)
  ) {
    const dow = parseInt(dayOfWeek, 10);
    const dayName = DAYS_OF_WEEK[dow] ?? `day ${dow}`;
    return `Every ${dayName} at ${formatTime(parseInt(hour, 10), parseInt(minute, 10))}`;
  }

  // Monthly on DOM at HH:MM: MM HH DOM * *
  if (
    isNumeric(minute) &&
    isNumeric(hour) &&
    isNumeric(dayOfMonth) &&
    month === '*' &&
    dayOfWeek === '*'
  ) {
    const dom = parseInt(dayOfMonth, 10);
    const suffix = dom === 1 ? 'st' : dom === 2 ? 'nd' : dom === 3 ? 'rd' : 'th';
    return `Monthly on the ${dom}${suffix} at ${formatTime(parseInt(hour, 10), parseInt(minute, 10))}`;
  }

  return expr;
}

/**
 * Calculates the next run time from a cron expression.
 * Handles common patterns. Returns null for expressions too complex to parse.
 */
export function getNextRun(expr: string, now?: Date): Date | null {
  const fields = parseCron(expr);
  if (!fields) return null;

  const { minute, hour, dayOfMonth, month, dayOfWeek } = fields;
  const base = now ?? new Date();

  // Every minute: * * * * *
  if (minute === '*' && hour === '*' && dayOfMonth === '*' && month === '*' && dayOfWeek === '*') {
    const next = new Date(base);
    next.setSeconds(0, 0);
    next.setMinutes(next.getMinutes() + 1);
    return next;
  }

  // Every N minutes: */N * * * *
  if (isStep(minute) && hour === '*' && dayOfMonth === '*' && month === '*' && dayOfWeek === '*') {
    const n = stepValue(minute);
    const next = new Date(base);
    next.setSeconds(0, 0);
    const currentMinute = next.getMinutes();
    const nextMinute = Math.ceil((currentMinute + 1) / n) * n;
    if (nextMinute >= 60) {
      next.setHours(next.getHours() + 1);
      next.setMinutes(0);
    } else {
      next.setMinutes(nextMinute);
    }
    return next;
  }

  // Every hour at :MM
  if (
    isNumeric(minute) &&
    hour === '*' &&
    dayOfMonth === '*' &&
    month === '*' &&
    dayOfWeek === '*'
  ) {
    const m = parseInt(minute, 10);
    const next = new Date(base);
    next.setSeconds(0, 0);
    next.setMinutes(m);
    if (next <= base) {
      next.setHours(next.getHours() + 1);
    }
    return next;
  }

  // Every N hours: MM */N * * *
  if (
    isNumeric(minute) &&
    isStep(hour) &&
    dayOfMonth === '*' &&
    month === '*' &&
    dayOfWeek === '*'
  ) {
    const m = parseInt(minute, 10);
    const n = stepValue(hour);
    const next = new Date(base);
    next.setSeconds(0, 0);
    next.setMinutes(m);
    const currentHour = next.getHours();
    const nextHour = Math.ceil((currentHour + (base >= next ? 1 : 0)) / n) * n;
    if (nextHour >= 24) {
      next.setDate(next.getDate() + 1);
      next.setHours(0);
    } else {
      next.setHours(nextHour);
    }
    return next;
  }

  // Daily at HH:MM
  if (
    isNumeric(minute) &&
    isNumeric(hour) &&
    dayOfMonth === '*' &&
    month === '*' &&
    dayOfWeek === '*'
  ) {
    const h = parseInt(hour, 10);
    const m = parseInt(minute, 10);
    const next = new Date(base);
    next.setSeconds(0, 0);
    next.setHours(h);
    next.setMinutes(m);
    if (next <= base) {
      next.setDate(next.getDate() + 1);
    }
    return next;
  }

  // Weekly on DOW at HH:MM
  if (
    isNumeric(minute) &&
    isNumeric(hour) &&
    dayOfMonth === '*' &&
    month === '*' &&
    isNumeric(dayOfWeek)
  ) {
    const h = parseInt(hour, 10);
    const m = parseInt(minute, 10);
    const dow = parseInt(dayOfWeek, 10);
    const next = new Date(base);
    next.setSeconds(0, 0);
    next.setHours(h);
    next.setMinutes(m);
    const currentDow = next.getDay();
    let daysAhead = dow - currentDow;
    if (daysAhead < 0 || (daysAhead === 0 && next <= base)) {
      daysAhead += 7;
    }
    next.setDate(next.getDate() + daysAhead);
    return next;
  }

  return null;
}

/**
 * Validates that a string is a valid 5-field cron expression.
 * Checks basic syntax: 5 space-separated fields, each containing
 * digits, *, /, -, or , characters.
 */
export function isValidCron(expr: string): boolean {
  const parts = expr.trim().split(/\s+/);
  if (parts.length !== 5) return false;
  const fieldPattern = /^(\*|\d+)(\/\d+)?(-\d+)?(,(\*|\d+)(\/\d+)?(-\d+)?)*$/;
  return parts.every((p) => fieldPattern.test(p));
}

export interface CronPreset {
  label: string;
  value: string;
}

export const CRON_PRESETS: CronPreset[] = [
  { label: 'Every hour', value: '0 * * * *' },
  { label: 'Every 6 hours', value: '0 */6 * * *' },
  { label: 'Daily at midnight', value: '0 0 * * *' },
  { label: 'Daily at 3am', value: '0 3 * * *' },
  { label: 'Weekly Monday', value: '0 0 * * 1' },
];
