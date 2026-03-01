import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { getToastMessage, isToastVisible, showToast, hideToast } from './notifications.svelte';

beforeEach(() => {
  vi.useFakeTimers();
  hideToast();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('notifications store', () => {
  it('starts with toast hidden and empty message', () => {
    expect(isToastVisible()).toBe(false);
    expect(getToastMessage()).toBe('');
  });

  it('showToast sets message and makes toast visible', () => {
    showToast('Session completed');

    expect(isToastVisible()).toBe(true);
    expect(getToastMessage()).toBe('Session completed');
  });

  it('auto-hides toast after default duration', () => {
    showToast('test');

    expect(isToastVisible()).toBe(true);

    vi.advanceTimersByTime(3000);

    expect(isToastVisible()).toBe(false);
  });

  it('auto-hides toast after custom duration', () => {
    showToast('test', 1000);

    vi.advanceTimersByTime(999);
    expect(isToastVisible()).toBe(true);

    vi.advanceTimersByTime(1);
    expect(isToastVisible()).toBe(false);
  });

  it('resets timer when showing new toast', () => {
    showToast('first', 2000);

    vi.advanceTimersByTime(1500);
    showToast('second', 2000);

    vi.advanceTimersByTime(1500);
    // First toast would have expired, but second should still be visible
    expect(isToastVisible()).toBe(true);
    expect(getToastMessage()).toBe('second');

    vi.advanceTimersByTime(500);
    expect(isToastVisible()).toBe(false);
  });

  it('hideToast immediately hides', () => {
    showToast('test');
    expect(isToastVisible()).toBe(true);

    hideToast();
    expect(isToastVisible()).toBe(false);
  });

  it('hideToast clears pending timer', () => {
    showToast('test', 5000);
    hideToast();

    // Show again — should auto-hide normally
    showToast('new', 1000);
    vi.advanceTimersByTime(1000);
    expect(isToastVisible()).toBe(false);
  });
});
