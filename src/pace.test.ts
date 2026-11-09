import { describe, expect, it } from 'vitest';
import { paceStatus, etaText, humanizeReset } from './pace';
import type { UsageWindow } from './types';

function w(p: Partial<UsageWindow>): UsageWindow {
  return {
    id: '5h', title: '5h', windowMinutes: 300, usedPercent: 0, quota: null,
    resetsAt: null, paceExpectedPercent: null, isKnown: true, ...p,
  };
}

describe('paceStatus', () => {
  it('hot when used exceeds expected by >2', () =>
    expect(paceStatus(w({ usedPercent: 88, paceExpectedPercent: 80 }))).toBe('hot'));
  it('under when below by >2', () =>
    expect(paceStatus(w({ usedPercent: 42, paceExpectedPercent: 58 }))).toBe('under'));
  it('on within threshold', () =>
    expect(paceStatus(w({ usedPercent: 50, paceExpectedPercent: 51 }))).toBe('on'));
  it('null without expected', () =>
    expect(paceStatus(w({ usedPercent: 42 }))).toBe(null));
});

describe('etaText', () => {
  it('computes runout from rate since window start', () => {
    const halfWindow = w({
      usedPercent: 50, paceExpectedPercent: 50, windowMinutes: 300,
      resetsAt: new Date(Date.now() + 150 * 60_000).toISOString(),
    });
    // 50% used in half the window ⇒ another 50% in the same span
    expect(etaText(halfWindow)).toMatch(/^~2h (29|30)m$/);
  });
  it('null when nothing consumed', () =>
    expect(etaText(w({ usedPercent: 0, resetsAt: new Date(Date.now() + 3_600_000).toISOString() }))).toBe(null));
  it('null without resetsAt', () => expect(etaText(w({ usedPercent: 50 }))).toBe(null));
});

describe('humanizeReset', () => {
  it('minutes only under an hour', () =>
    expect(humanizeReset(new Date(Date.now() + 45 * 60_000).toISOString())).toBe('45m'));
  it('hours and minutes', () =>
    expect(humanizeReset(new Date(Date.now() + 100 * 60_000).toISOString())).toMatch(/^1h 4[0-5]m$/));
  it('days and hours beyond a day', () =>
    expect(humanizeReset(new Date(Date.now() + 8640 * 60_000).toISOString())).toMatch(/^6d 0h$/));
});
