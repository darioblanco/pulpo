import { describe, it, expect } from 'vitest';
import { describeCron, getNextRun, isValidCron, CRON_PRESETS } from './cron';

describe('describeCron', () => {
  it('describes every minute', () => {
    expect(describeCron('* * * * *')).toBe('Every minute');
  });

  it('describes every N minutes', () => {
    expect(describeCron('*/5 * * * *')).toBe('Every 5 minutes');
    expect(describeCron('*/1 * * * *')).toBe('Every minute');
    expect(describeCron('*/15 * * * *')).toBe('Every 15 minutes');
  });

  it('describes every hour', () => {
    expect(describeCron('0 * * * *')).toBe('Every hour');
  });

  it('describes every hour at specific minute', () => {
    expect(describeCron('30 * * * *')).toBe('Every hour at :30');
    expect(describeCron('5 * * * *')).toBe('Every hour at :05');
  });

  it('describes every N hours', () => {
    expect(describeCron('0 */6 * * *')).toBe('Every 6 hours');
    expect(describeCron('0 */1 * * *')).toBe('Every hour');
  });

  it('describes daily at specific time', () => {
    expect(describeCron('0 3 * * *')).toBe('Every day at 3:00 AM');
    expect(describeCron('30 14 * * *')).toBe('Every day at 2:30 PM');
    expect(describeCron('0 0 * * *')).toBe('Every day at 12:00 AM');
    expect(describeCron('0 12 * * *')).toBe('Every day at 12:00 PM');
  });

  it('describes weekly on specific day', () => {
    expect(describeCron('0 0 * * 1')).toBe('Every Monday at 12:00 AM');
    expect(describeCron('30 9 * * 0')).toBe('Every Sunday at 9:30 AM');
    expect(describeCron('0 17 * * 5')).toBe('Every Friday at 5:00 PM');
  });

  it('describes monthly on specific day', () => {
    expect(describeCron('0 3 1 * *')).toBe('Monthly on the 1st at 3:00 AM');
    expect(describeCron('0 3 2 * *')).toBe('Monthly on the 2nd at 3:00 AM');
    expect(describeCron('0 3 3 * *')).toBe('Monthly on the 3rd at 3:00 AM');
    expect(describeCron('0 3 15 * *')).toBe('Monthly on the 15th at 3:00 AM');
  });

  it('returns raw expression for complex crons', () => {
    expect(describeCron('0 3 * * 1,3,5')).toBe('0 3 * * 1,3,5');
    expect(describeCron('0 3 1-15 * *')).toBe('0 3 1-15 * *');
  });

  it('returns raw expression for invalid input', () => {
    expect(describeCron('not a cron')).toBe('not a cron');
    expect(describeCron('')).toBe('');
  });
});

describe('getNextRun', () => {
  const base = new Date('2026-03-20T10:30:00');

  it('returns next minute for * * * * *', () => {
    const next = getNextRun('* * * * *', base);
    expect(next).not.toBeNull();
    expect(next!.getMinutes()).toBe(31);
    expect(next!.getHours()).toBe(10);
  });

  it('returns next N-minute boundary for */N', () => {
    const next = getNextRun('*/15 * * * *', base);
    expect(next).not.toBeNull();
    expect(next!.getMinutes()).toBe(45);
  });

  it('returns next hour for hourly cron', () => {
    const next = getNextRun('0 * * * *', base);
    expect(next).not.toBeNull();
    expect(next!.getHours()).toBe(11);
    expect(next!.getMinutes()).toBe(0);
  });

  it('returns same hour if minute not yet passed', () => {
    const before = new Date('2026-03-20T10:15:00');
    const next = getNextRun('30 * * * *', before);
    expect(next).not.toBeNull();
    expect(next!.getHours()).toBe(10);
    expect(next!.getMinutes()).toBe(30);
  });

  it('returns next day for daily cron if time passed', () => {
    const next = getNextRun('0 3 * * *', base);
    expect(next).not.toBeNull();
    expect(next!.getDate()).toBe(21);
    expect(next!.getHours()).toBe(3);
  });

  it('returns same day for daily cron if time not passed', () => {
    const morning = new Date('2026-03-20T02:00:00');
    const next = getNextRun('0 3 * * *', morning);
    expect(next).not.toBeNull();
    expect(next!.getDate()).toBe(20);
    expect(next!.getHours()).toBe(3);
  });

  it('returns next matching weekday for weekly cron', () => {
    // 2026-03-20 is a Friday (day 5)
    const next = getNextRun('0 0 * * 1', base); // Monday
    expect(next).not.toBeNull();
    expect(next!.getDay()).toBe(1);
    expect(next!.getDate()).toBe(23); // Next Monday
  });

  it('returns null for complex expressions', () => {
    expect(getNextRun('0 3 * * 1,3,5', base)).toBeNull();
    expect(getNextRun('invalid', base)).toBeNull();
  });

  it('handles every N hours', () => {
    const next = getNextRun('0 */6 * * *', base);
    expect(next).not.toBeNull();
    expect(next!.getHours()).toBe(12);
    expect(next!.getMinutes()).toBe(0);
  });
});

describe('isValidCron', () => {
  it('accepts valid 5-field expressions', () => {
    expect(isValidCron('* * * * *')).toBe(true);
    expect(isValidCron('0 3 * * *')).toBe(true);
    expect(isValidCron('*/15 * * * *')).toBe(true);
    expect(isValidCron('0 0 * * 1')).toBe(true);
    expect(isValidCron('0 */6 * * *')).toBe(true);
  });

  it('rejects invalid expressions', () => {
    expect(isValidCron('not cron')).toBe(false);
    expect(isValidCron('')).toBe(false);
    expect(isValidCron('* * *')).toBe(false);
    expect(isValidCron('* * * * * *')).toBe(false);
  });
});

describe('CRON_PRESETS', () => {
  it('has expected presets', () => {
    expect(CRON_PRESETS.length).toBe(5);
    expect(CRON_PRESETS.map((p) => p.label)).toContain('Every hour');
    expect(CRON_PRESETS.map((p) => p.label)).toContain('Daily at 3am');
  });

  it('all presets are valid cron expressions', () => {
    for (const preset of CRON_PRESETS) {
      expect(isValidCron(preset.value)).toBe(true);
    }
  });

  it('all presets have human-readable descriptions', () => {
    for (const preset of CRON_PRESETS) {
      const desc = describeCron(preset.value);
      expect(desc).not.toBe(preset.value); // Should not fall back to raw expression
    }
  });
});
